package com.uscreen

import android.media.MediaCodec
import android.media.MediaCrypto
import android.media.MediaFormat
import android.os.Handler
import android.os.HandlerThread
import android.view.Surface
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.Implementation
import org.robolectric.annotation.Implements
import org.robolectric.shadows.ShadowMediaCodec

@Suppress("UNUSED_PARAMETER")
@Implements(MediaCodec::class)
class FailingCodecShadow : ShadowMediaCodec() {
    companion object {
        var failAt = "configure"
        var releases = 0
        var lastFormat: MediaFormat? = null
    }
    @Implementation fun configure(format: MediaFormat, surface: Surface?, crypto: MediaCrypto?, flags: Int) {
        lastFormat = format
        if (failAt == "configure") throw IllegalStateException("injected configure failure")
    }
    @Implementation fun setVideoScalingMode(mode: Int) {}
    @Implementation fun setOnFrameRenderedListener(listener: MediaCodec.OnFrameRenderedListener, handler: Handler) {
        if (failAt == "listener") throw IllegalStateException("injected listener failure")
    }
    @Implementation fun start() {
        if (failAt == "start") throw IllegalStateException("injected start failure")
    }
    @Implementation fun stop() {}
    @Implementation fun release() { releases++ }
    @Implementation fun dequeueInputBuffer(timeoutUs: Long): Int {
        if (failAt == "input-timeout") return -1
        if (failAt == "feed-error") throw IllegalStateException("injected feed failure")
        return 0
    }
    @Implementation fun getInputBuffer(index: Int): java.nio.ByteBuffer? =
        if (failAt == "null-input") null else java.nio.ByteBuffer.allocate(1)
    @Implementation fun queueInputBuffer(index: Int, offset: Int, size: Int, presentationTimeUs: Long, flags: Int) {}
    @Implementation fun dequeueOutputBuffer(info: MediaCodec.BufferInfo, timeoutUs: Long): Int {
        throw IllegalStateException("injected output failure")
    }
}

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34], shadows = [FailingCodecShadow::class])
class DecoderSetupTest {
    @get:org.junit.Rule val decoderInventory = DecoderInventoryRule()
    private fun owner(target: Any, name: String): Any = when {
        target is VideoReceiver && name == "socket" -> target.transport
        target is VideoReceiver && name in setOf("mediaCodec", "codecAlive", "outputThread") -> target.decoder
        else -> target
    }
    private fun set(target: Any, name: String, value: Any?) {
        val owner = owner(target, name)
        owner.javaClass.getDeclaredField(name).apply { isAccessible = true }.set(owner, value)
    }
    private fun get(target: Any, name: String): Any? {
        val owner = owner(target, name)
        return owner.javaClass.getDeclaredField(name).apply { isAccessible = true }.get(owner)
    }


    @Test fun t404_stalePacketCannotRecordArrivalInCurrentTimingEpoch() {
        val receiver = VideoReceiver()
        val codec = MediaCodec.createDecoderByType(VideoReceiver.MIME_TYPE)
        val packetType = Class.forName("com.uscreen.VideoReceiver\$VideoPackets")
        val constructor = packetType.getDeclaredConstructor(VideoReceiver::class.java, Long::class.javaPrimitiveType)
            .apply { isAccessible = true }
        val packets = constructor.newInstance(receiver, 0L)
        packetType.getDeclaredField("codec").apply { isAccessible = true }.set(packets, codec)
        val frame = packetType.getDeclaredMethod("frame", Int::class.javaPrimitiveType, ByteArray::class.java,
            Int::class.javaPrimitiveType, Int::class.javaPrimitiveType).apply { isAccessible = true }
        try {
            // Dispatch models retirement between read validation and the packet
            // callback. The stopped receiver must not accept timing publication.
            frame.invoke(packets, 71, byteArrayOf(1), 0, 1)
            assertEquals("T404: stale packet wrote into current history", -1, receiver.timing.decodeMicrosFor(71))
        } finally { receiver.stop(); codec.release() }
    }

    @Test fun t243_invalidPacketReportsDisconnectionBeforeRetry() = checkDisconnected("invalid-packet")
    @Test fun t243_decoderResetReportsDisconnectionBeforeRetry() = checkDisconnected("input-timeout")

    private class ConnectingSocket : java.net.Socket() {
        val entered = java.util.concurrent.CountDownLatch(1)
        private val released = java.util.concurrent.CountDownLatch(1)
        @Volatile var deadline = -1
        override fun connect(endpoint: java.net.SocketAddress?, timeout: Int) {
            deadline = timeout
            entered.countDown()
            check(released.await(5, java.util.concurrent.TimeUnit.SECONDS))
            throw java.net.SocketException("fixture connection closed")
        }
        override fun close() { super.close(); released.countDown() }
    }

    @Test fun t333_stopCancelsPendingVideoConnect() = checkPendingConnect(true)
    @Test fun t333_videoConnectHasABoundedDeadline() = checkPendingConnect(false)

    private fun checkPendingConnect(checkCancellation: Boolean) {
        val socket = ConnectingSocket()
        val receiver = VideoReceiver { socket }
        (get(receiver, "surfaceReady") as java.util.concurrent.atomic.AtomicBoolean).set(true)
        set(receiver, "mediaCodec", MediaCodec.createDecoderByType(VideoReceiver.MIME_TYPE))
        var job: kotlinx.coroutines.Job? = null
        try {
            receiver.start()
            job = get(receiver, "job") as kotlinx.coroutines.Job
            assertTrue("T333: connect was not attempted",
                socket.entered.await(2, java.util.concurrent.TimeUnit.SECONDS))
            if (checkCancellation) {
                receiver.stop()
                val retired = kotlinx.coroutines.runBlocking {
                    kotlinx.coroutines.withTimeoutOrNull(1000) { job.join(); true } ?: false
                }
                assertTrue("T333: stopping left the video connection worker blocked", retired)
                assertTrue("T333: pending socket was not closed", socket.isClosed)
            } else {
                assertTrue("T333: connect has no bounded deadline: ${socket.deadline}",
                    socket.deadline in 1..10_000)
            }
        } finally {
            receiver.stop()
            socket.close()
            kotlinx.coroutines.runBlocking { kotlinx.coroutines.withTimeout(2000) { job?.join() } }
        }
    }

    private class RecoverySocket(private val bytes: ByteArray?, private val holdAfterBytes: Boolean = false) : java.net.Socket() {
        private val releaseRead = java.util.concurrent.CountDownLatch(1)
        val blockedRead = java.util.concurrent.CountDownLatch(1)
        @Volatile private var closed = false
        override fun connect(endpoint: java.net.SocketAddress?, timeout: Int) {}
        override fun setTcpNoDelay(value: Boolean) {}
        override fun setSoTimeout(value: Int) {}
        override fun setReceiveBufferSize(value: Int) {}
        override fun isClosed() = closed
        override fun getInputStream(): java.io.InputStream {
            val idle = object : java.io.InputStream() {
                override fun read(): Int {
                    blockedRead.countDown()
                    check(releaseRead.await(5, java.util.concurrent.TimeUnit.SECONDS))
                    throw java.io.EOFException()
                }
            }
            val initial = bytes?.inputStream() ?: return idle
            return if (holdAfterBytes) java.io.SequenceInputStream(initial, idle) else initial
        }
        override fun close() { closed = true; releaseRead.countDown() }
    }

    @Test fun t329_firstVideoFrameSurvivesLateUiSubscriptionAndStop() = checkLateSubscription(true)
    @Test fun t329_firstVideoFrameSurvivesLateUiSubscriptionAndDisconnect() = checkLateSubscription(false)

    private fun checkLateSubscription(stop: Boolean) {
        val socket = RecoverySocket(byteArrayOf(0, 0, 0, 6, 1, 0, 0, 0, 1, 42), true)
        val receiver = VideoReceiver { socket }
        FailingCodecShadow.failAt = "none"
        (get(receiver, "surfaceReady") as java.util.concurrent.atomic.AtomicBoolean).set(true)
        set(receiver, "mediaCodec", MediaCodec.createDecoderByType(VideoReceiver.MIME_TYPE))
        val connected = java.util.concurrent.atomic.AtomicInteger()
        val disconnected = java.util.concurrent.atomic.AtomicInteger()
        val disconnectedEvent = java.util.concurrent.CountDownLatch(1)
        var job: kotlinx.coroutines.Job? = null
        try {
            receiver.start()
            job = get(receiver, "job") as? kotlinx.coroutines.Job
            assertTrue("T329: first frame was not consumed",
                socket.blockedRead.await(2, java.util.concurrent.TimeUnit.SECONDS))
            val onConnected = { connected.incrementAndGet(); Unit }
            val onDisconnected = { disconnected.incrementAndGet(); disconnectedEvent.countDown() }
            receiver.observeConnection(onConnected, onDisconnected)
            assertEquals("T329: the UI missed a frame received before its listener", 1, connected.get())
            assertEquals(0, disconnected.get())
            if (stop) receiver.stop() else socket.close()
            assertTrue("T329: stream retirement was not reported",
                disconnectedEvent.await(2, java.util.concurrent.TimeUnit.SECONDS))
            assertEquals(1, disconnected.get())
            receiver.observeConnection(onConnected, onDisconnected)
            assertEquals("T329: retired readiness was replayed", 1, connected.get())
            assertEquals("T329: new observer did not see retirement", 2, disconnected.get())
        } finally {
            receiver.stop()
            socket.close()
            kotlinx.coroutines.runBlocking { kotlinx.coroutines.withTimeout(2000) { job?.join() } }
        }
    }

    private fun checkDisconnected(stage: String) {
        // One frame followed by an unknown packet. The timeout variant resets
        // the decoder while feeding that frame and returns before the next read.
        val original = RecoverySocket(byteArrayOf(0, 0, 0, 6, 1, 0, 0, 0, 1, 42, 0, 0, 0, 2, 99, 0))
        val replacement = RecoverySocket(null)
        val opens = java.util.concurrent.atomic.AtomicInteger()
        val disconnected = java.util.concurrent.CountDownLatch(1)
        val connected = java.util.concurrent.atomic.AtomicBoolean()
        val receiver = VideoReceiver { if (opens.getAndIncrement() == 0) original else replacement }
        FailingCodecShadow.failAt = "none"
        (get(receiver, "surfaceReady") as java.util.concurrent.atomic.AtomicBoolean).set(true)
        set(receiver, "mediaCodec", MediaCodec.createDecoderByType(VideoReceiver.MIME_TYPE))
        receiver.onConnected = {
            connected.set(true)
            if (stage == "input-timeout") FailingCodecShadow.failAt = stage
        }
        receiver.onDisconnected = { connected.set(false); disconnected.countDown() }
        try {
            receiver.start()
            assertTrue("T243: $stage retained a connected UI after stream retirement",
                disconnected.await(2, java.util.concurrent.TimeUnit.SECONDS))
            assertFalse("A replacement without a frame must remain disconnected", connected.get())
        } finally {
            val job = get(receiver, "job") as? kotlinx.coroutines.Job
            receiver.stop()
            original.close()
            replacement.close()
            kotlinx.coroutines.runBlocking { kotlinx.coroutines.withTimeout(2000) { job?.join() } }
        }
    }

    @Test fun t134_inputTimeoutReconnects() = checkRecovery("input-timeout")
    @Test fun t134_feedErrorReconnects() = checkRecovery("feed-error")
    @Test fun t134_oversizedInputReconnects() = checkRecovery("oversized-input")
    @Test fun t134_nullInputReconnects() = checkRecovery("null-input")
    @Test fun t134_surfaceReplacementReconnects() = checkRecovery("surface")

    private fun checkRecovery(stage: String) {
        run {
            val receiver = VideoReceiver()
            val socket = java.net.Socket()
            val surface = Surface(android.graphics.SurfaceTexture(1))
            val codec = MediaCodec.createDecoderByType(VideoReceiver.MIME_TYPE)
            FailingCodecShadow.failAt = stage
            FailingCodecShadow.releases = 0
            set(receiver, "mediaCodec", codec)
            set(receiver, "socket", socket)
            set(receiver, "isRunning", true)
            @Suppress("UNCHECKED_CAST")
            val pending = get(receiver, "pendingSurface") as java.util.concurrent.atomic.AtomicReference<Surface?>
            pending.set(surface)
            try {
                if (stage == "surface") receiver.onSurfaceDestroyed()
                else receiver.feedDecoder(0L, codec, byteArrayOf(1, 2), 0, 2, false, 1L)
                assertTrue("$stage retained a stream without fresh codec headers", socket.isClosed)
                assertNull("$stage must let reconnect create the replacement decoder", get(receiver, "mediaCodec"))
                assertEquals("$stage leaked or double-released its codec", 1, FailingCodecShadow.releases)
            } finally { receiver.stop(); surface.release(); socket.close() }
        }
    }

    @Test fun t134_outputFailureRetiresStreamWithoutWaitingForInput() {
        val receiver = VideoReceiver()
        val socket = java.net.Socket()
        val codec = MediaCodec.createDecoderByType(VideoReceiver.MIME_TYPE)
        set(receiver, "mediaCodec", codec)
        set(receiver, "socket", socket)
        set(receiver, "codecAlive", true)
        try {
            receiver.decoder.startOutputThread(codec)
            (get(receiver, "outputThread") as? Thread)?.join(2000)
            assertTrue("Failed output thread retained the stream", socket.isClosed)
            assertNull(get(receiver, "mediaCodec"))
        } finally { receiver.stop(); socket.close() }
    }

    @Test fun t120_decoderHintsUseTheEffectiveRateAcrossRestarts() {
        val receiver = VideoReceiver()
        val surface = Surface(android.graphics.SurfaceTexture(1))
        FailingCodecShadow.failAt = "start"
        try {
            for (fps in listOf(30, 90, 60)) {
                receiver.streamFps = fps
                assertEquals(false, receiver.setupCodec(surface))
                assertEquals(fps, FailingCodecShadow.lastFormat!!.getInteger(MediaFormat.KEY_FRAME_RATE))
                assertEquals(fps * 2, FailingCodecShadow.lastFormat!!.getInteger("operating-rate"))
                receiver.stop()
            }
        } finally { receiver.stop(); surface.release() }
    }

    @Test fun t089_configureFailureReleasesResources() = checkFailure("configure")
    @Test fun t089_listenerFailureReleasesResources() = checkFailure("listener")
    @Test fun t089_startFailureReleasesResources() = checkFailure("start")

    private fun checkFailure(stage: String) {
        FailingCodecShadow.failAt = stage
        FailingCodecShadow.releases = 0
        val threads = mutableListOf<HandlerThread>()
        var quits = 0
        val receiver = VideoReceiver()
        receiver.decoder.callbackThreadFactory = {
            object : HandlerThread("uscreen-test-frame-cb") {
                override fun quitSafely(): Boolean { quits++; return super.quitSafely() }
            }.also { threads.add(it) }
        }
        val surface = Surface(android.graphics.SurfaceTexture(1))
        try {
            for (attempt in 1..2) {
                assertEquals(false, receiver.setupCodec(surface))
                assertEquals("$stage leaked codec on attempt $attempt", attempt, FailingCodecShadow.releases)
                assertEquals(threads.size, quits)
                threads.forEach { it.join(500); assertFalse("Callback thread leaked", it.isAlive) }
            }
            receiver.stop()
            assertEquals("Failed resources must not be released twice", 2, FailingCodecShadow.releases)
            assertEquals(threads.size, quits)
        } finally {
            receiver.stop()
            threads.filter { it.isAlive }.forEach { it.quitSafely(); it.join(500) }
            surface.release()
        }
    }
}
