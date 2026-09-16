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
    @Implementation fun dequeueOutputBuffer(info: MediaCodec.BufferInfo, timeoutUs: Long): Int {
        throw IllegalStateException("injected output failure")
    }
}

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34], shadows = [FailingCodecShadow::class])
class DecoderSetupTest {
    private fun set(target: Any, name: String, value: Any?) = target.javaClass.getDeclaredField(name).apply { isAccessible = true }.set(target, value)
    private fun get(target: Any, name: String): Any? = target.javaClass.getDeclaredField(name).apply { isAccessible = true }.get(target)

    @Test fun t134_inputTimeoutReconnects() = checkRecovery("input-timeout")
    @Test fun t134_feedErrorReconnects() = checkRecovery("feed-error")
    @Test fun t134_oversizedInputReconnects() = checkRecovery("oversized-input")
    @Test fun t134_nullInputReconnects() = checkRecovery("null-input")
    @Test fun t134_surfaceReplacementReconnects() = checkRecovery("surface")

    private fun checkRecovery(stage: String) {
        val feed = VideoReceiver::class.java.getDeclaredMethod("feedDecoder",
            Long::class.javaPrimitiveType, MediaCodec::class.java, ByteArray::class.java,
            Int::class.javaPrimitiveType, Int::class.javaPrimitiveType,
            Boolean::class.javaPrimitiveType, Long::class.javaPrimitiveType).apply { isAccessible = true }
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
                else feed.invoke(receiver, 0L, codec, byteArrayOf(1, 2), 0, 2, false, 1L)
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
        val start = VideoReceiver::class.java.getDeclaredMethod("startOutputThread", MediaCodec::class.java).apply { isAccessible = true }
        try {
            start.invoke(receiver, codec)
            (get(receiver, "outputThread") as? Thread)?.join(2000)
            assertTrue("Failed output thread retained the stream", socket.isClosed)
            assertNull(get(receiver, "mediaCodec"))
        } finally { receiver.stop(); socket.close() }
    }

    @Test fun t120_decoderHintsUseTheEffectiveRateAcrossRestarts() {
        val receiver = VideoReceiver()
        val setup = VideoReceiver::class.java.getDeclaredMethod("setupCodec", Surface::class.java).apply { isAccessible = true }
        val surface = Surface(android.graphics.SurfaceTexture(1))
        FailingCodecShadow.failAt = "start"
        try {
            for (fps in listOf(30, 90, 60)) {
                receiver.streamFps = fps
                assertEquals(false, setup.invoke(receiver, surface))
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
        receiver.callbackThreadFactory = {
            object : HandlerThread("uscreen-test-frame-cb") {
                override fun quitSafely(): Boolean { quits++; return super.quitSafely() }
            }.also { threads.add(it) }
        }
        val setup = VideoReceiver::class.java.getDeclaredMethod("setupCodec", Surface::class.java).apply { isAccessible = true }
        val surface = Surface(android.graphics.SurfaceTexture(1))
        try {
            for (attempt in 1..2) {
                assertEquals(false, setup.invoke(receiver, surface))
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
