package com.uscreen

import android.graphics.SurfaceTexture
import android.media.MediaCodec
import android.media.MediaCrypto
import android.media.MediaFormat
import android.os.Handler
import android.view.Surface
import java.net.Socket
import java.nio.ByteBuffer
import java.util.concurrent.CountDownLatch
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicLong
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
class WatchdogCodecShadow : ShadowMediaCodec() {
    data class Output(val sequence: Int, val barrier: CountDownLatch? = null)
    companion object {
        val outputs = LinkedBlockingQueue<Output>()
        var format: MediaFormat? = null
        val callbacks = java.util.concurrent.CopyOnWriteArrayList<MediaCodec.OnFrameRenderedListener>()
    }
    @Implementation fun configure(value: MediaFormat, surface: Surface?, crypto: MediaCrypto?, flags: Int) { format = value }
    @Implementation fun setVideoScalingMode(mode: Int) {}
    @Implementation fun setOnFrameRenderedListener(listener: MediaCodec.OnFrameRenderedListener, handler: Handler) {
        callbacks.add(listener)
    }
    @Implementation fun start() {}
    @Implementation fun stop() {}
    @Implementation fun release() {}
    @Implementation fun dequeueInputBuffer(timeoutUs: Long) = 0
    @Implementation fun getInputBuffer(index: Int): ByteBuffer = ByteBuffer.allocate(1024)
    @Implementation fun queueInputBuffer(index: Int, offset: Int, size: Int, presentationTimeUs: Long, flags: Int) {}
    @Implementation override fun releaseOutputBuffer(index: Int, render: Boolean) {}
    @Implementation fun dequeueOutputBuffer(info: MediaCodec.BufferInfo, timeoutUs: Long): Int {
        val output = outputs.poll(10, TimeUnit.MILLISECONDS) ?: return -1
        output.barrier?.let { it.countDown(); return -1 }
        info.presentationTimeUs = output.sequence.toLong()
        return 0
    }
}

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34], shadows = [WatchdogCodecShadow::class])
class DecoderWatchdogTest {
    @Test fun t376_retiredCodecCallbacksCannotAcknowledgeReplacement() {
        WatchdogCodecShadow.outputs.clear()
        WatchdogCodecShadow.callbacks.clear()
        val monitor = Any()
        var running = true
        val acknowledgements = mutableListOf<Int>()
        var retirements = 0
        val statistics = ReceiverStatistics()
        val decoder = DecoderSession(monitor, { running }, FrameTiming(), object : DecoderEvents {
            override fun rendered(sequence: Int, decodeMicros: Int) { acknowledgements.add(sequence) }
            override fun invalidated() { retirements++ }
        }, { statistics })
        val formats = mutableListOf<String>()
        decoder.createCodec = { formats.add(it); MediaCodec.createDecoderByType(it) }
        val surface = Surface(SurfaceTexture(1))
        val format = DecoderFormat(VideoReceiver.MIME_TYPE, 1280, 800, 60)
        try {
            assertTrue(decoder.setupCodec(surface, format))
            val oldCodec = decoder.mediaCodec!!
            val oldCallback = WatchdogCodecShadow.callbacks.last()
            decoder.resetCodec()
            assertEquals(1, retirements)
            assertTrue(surface.isValid)
            assertTrue(decoder.setupCodec(surface, format))
            val current = decoder.mediaCodec!!
            val callback = WatchdogCodecShadow.callbacks.last()
            oldCallback.onFrameRendered(oldCodec, 17, 0)
            callback.onFrameRendered(current, 18, 0)
            running = false
            callback.onFrameRendered(current, 19, 0)
            assertEquals(listOf(18), acknowledgements)
            assertEquals(listOf(format.mimeType, format.mimeType), formats)
        } finally { decoder.releaseCodec(); surface.release() }
    }

    private class Fixture : AutoCloseable {
        val receiver = VideoReceiver()
        val now = AtomicLong(1_000_000_000)
        private val surface = Surface(SurfaceTexture(1))
        private var socket = Socket()
        init {
            WatchdogCodecShadow.outputs.clear()
            receiver.decoder.outputClock = now::get
        }
        private fun set(name: String, value: Any) = VideoTransport::class.java.getDeclaredField(name)
            .apply { isAccessible = true }.set(receiver.transport, value)
        fun restart() {
            socket.close()
            socket = Socket()
            set("socket", socket)
            assertEquals(true, receiver.setupCodec(surface))
        }
        fun output(elapsedNanos: Long = 0) {
            now.addAndGet(elapsedNanos)
            val barrier = CountDownLatch(1)
            WatchdogCodecShadow.outputs.put(WatchdogCodecShadow.Output(1))
            WatchdogCodecShadow.outputs.put(WatchdogCodecShadow.Output(-1, barrier))
            assertTrue("T339: output worker did not finish a frame", barrier.await(2, TimeUnit.SECONDS))
        }
        fun stall() {
            now.addAndGet(2_000_000_000)
            synchronized(receiver) { repeat(4) { receiver.decoder.checkOutputProgress() } }
            assertTrue("T339: watchdog did not retire the socket", socket.isClosed)
        }
        fun assertHints(enabled: Boolean) {
            val format = WatchdogCodecShadow.format!!
            assertEquals(enabled, format.containsKey("operating-rate"))
            assertEquals(enabled, format.containsKey("vendor.qti-ext-dec-low-latency.enable"))
            if (android.os.Build.VERSION.SDK_INT >= 30) {
                assertEquals(enabled, format.containsKey(MediaFormat.KEY_LOW_LATENCY))
            }
            assertEquals(receiver.streamFps, format.getInteger(MediaFormat.KEY_FRAME_RATE))
            assertEquals(receiver.mimeType, format.getString(MediaFormat.KEY_MIME))
        }
        override fun close() { receiver.stop(); socket.close(); surface.release() }
    }

    private fun repeatedStalls(frames: Int) {
        Fixture().use { fixture ->
            repeat(2) {
                fixture.restart()
                fixture.assertHints(true)
                repeat(frames) { fixture.output() }
                fixture.stall()
            }
            fixture.restart()
            fixture.assertHints(false)
        }
    }
    @Test fun t339_zeroFrameStallsReachFallback() = repeatedStalls(0)
    @Test fun t339_oneFrameStallsReachFallback() = repeatedStalls(1)

    @Test fun t339_sustainedOutputResetsStallHistory() {
        Fixture().use { fixture ->
            fixture.restart()
            fixture.stall()
            fixture.restart()
            fixture.output()
            repeat(3) { fixture.output(500_000_000) }
            fixture.stall()
            fixture.restart()
            fixture.assertHints(true)
            fixture.stall()
            fixture.restart()
            fixture.assertHints(false)
        }
    }

    @Test fun t339_aShortBurstIsNotSustainedRecovery() {
        Fixture().use { fixture ->
            fixture.restart()
            fixture.stall()
            fixture.restart()
            repeat(20) { fixture.output(1_000_000) }
            fixture.stall()
            fixture.restart()
            fixture.assertHints(false)
        }
    }

    @Test fun t339_gapsBetweenIsolatedFramesDoNotClearFailures() {
        Fixture().use { fixture ->
            fixture.restart()
            fixture.stall()
            fixture.restart()
            repeat(4) { fixture.output(2_000_000_000) }
            fixture.stall()
            fixture.restart()
            fixture.assertHints(false)
        }
    }

    @Test fun t339_eachReplacementNeedsItsOwnRecoveryWindow() {
        Fixture().use { fixture ->
            fixture.restart()
            fixture.stall()
            fixture.restart()
            fixture.output()
            repeat(2) { fixture.output(500_000_000) }
            fixture.receiver.stop()
            fixture.receiver.start()
            fixture.restart()
            fixture.output(600_000_000)
            fixture.stall()
            fixture.restart()
            fixture.assertHints(false)
        }
    }

    @Test fun t339_fallbackSurvivesRecoveryStopStartAndCodecChanges() {
        Fixture().use { fixture ->
            repeat(2) { fixture.restart(); fixture.stall() }
            fixture.restart()
            fixture.output()
            repeat(3) { fixture.output(500_000_000) }
            fixture.receiver.stop()
            fixture.receiver.mimeType = VideoReceiver.MIME_TYPE_HEVC
            fixture.receiver.start() // No Surface is published to the connection worker.
            fixture.restart()
            fixture.assertHints(false)
        }
    }
}
