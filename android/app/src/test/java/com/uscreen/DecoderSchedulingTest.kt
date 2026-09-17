package com.uscreen

import android.graphics.SurfaceTexture
import android.media.MediaCodec
import android.media.MediaCrypto
import android.media.MediaFormat
import android.os.Handler
import android.view.Surface
import java.nio.ByteBuffer
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference
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
class SchedulingCodecShadow : ShadowMediaCodec() {
    companion object {
        var entered = CountDownLatch(1)
        var resume = CountDownLatch(1)
        var released = CountDownLatch(1)
    }
    @Implementation fun configure(format: MediaFormat, surface: Surface?, crypto: MediaCrypto?, flags: Int) {}
    @Implementation fun setVideoScalingMode(mode: Int) {}
    @Implementation fun setOnFrameRenderedListener(listener: MediaCodec.OnFrameRenderedListener, handler: Handler) {}
    @Implementation fun start() {}
    @Implementation fun stop() {}
    @Implementation fun release() { released.countDown() }
    @Implementation fun dequeueInputBuffer(timeoutUs: Long): Int {
        entered.countDown()
        check(resume.await(5, TimeUnit.SECONDS))
        return 0
    }
    @Implementation fun getInputBuffer(index: Int): ByteBuffer = ByteBuffer.allocate(1024)
    @Implementation fun queueInputBuffer(index: Int, offset: Int, size: Int, presentationTimeUs: Long, flags: Int) {}
    @Implementation fun dequeueOutputBuffer(info: MediaCodec.BufferInfo, timeoutUs: Long): Int {
        Thread.sleep(10)
        return -1
    }
}

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34], shadows = [SchedulingCodecShadow::class])
class DecoderSchedulingTest {
    private class Fixture : AutoCloseable {
        val monitor = Any()
        private val surface = Surface(SurfaceTexture(1))
        private val failure = AtomicReference<Throwable?>()
        private var feed: Thread? = null
        val decoder = DecoderSession(monitor, { true }, FrameTiming(), object : DecoderEvents {
            override fun rendered(sequence: Int, decodeMicros: Int) {}
            override fun invalidated() {}
        }, { ReceiverStatistics() })

        init {
            SchedulingCodecShadow.entered = CountDownLatch(1)
            SchedulingCodecShadow.resume = CountDownLatch(1)
            SchedulingCodecShadow.released = CountDownLatch(1)
            assertTrue(decoder.setupCodec(surface, DecoderFormat(VideoReceiver.MIME_TYPE, 1280, 800, 60)))
        }
        fun blockedInput() {
            val codec = decoder.mediaCodec!!
            feed = Thread {
                try { decoder.feedDecoder(codec, byteArrayOf(7), 0, 1, false, 1) }
                catch (error: Throwable) { failure.set(error) }
            }.apply { start() }
            assertTrue(SchedulingCodecShadow.entered.await(2, TimeUnit.SECONDS))
        }
        override fun close() {
            SchedulingCodecShadow.resume.countDown()
            feed?.join(2_000)
            decoder.releaseCodec()
            surface.release()
            failure.get()?.let { throw AssertionError("T386 feed worker", it) }
        }
    }

    @Test fun t386_inputWaitDoesNotHoldReceiverMonitor() {
        Fixture().use { fixture ->
            fixture.blockedInput()
            val acquired = CountDownLatch(1)
            val worker = Thread { synchronized(fixture.monitor) { acquired.countDown() } }.apply { start() }
            try {
                assertTrue("T386: input wait monopolized receiver monitor", acquired.await(300, TimeUnit.MILLISECONDS))
            } finally {
                SchedulingCodecShadow.resume.countDown()
                worker.join(2_000)
            }
        }
    }

    @Test fun t386_retirementDoesNotWaitForBlockedNativeInput() {
        Fixture().use { fixture ->
            fixture.blockedInput()
            val retired = CountDownLatch(1)
            val worker = Thread { fixture.decoder.releaseCodec(); retired.countDown() }.apply { start() }
            try {
                assertTrue("T386: blocked native input prevented retirement", retired.await(750, TimeUnit.MILLISECONDS))
                assertNull(fixture.decoder.mediaCodec)
                assertEquals("T386: native storage released while input still owned", 1L, SchedulingCodecShadow.released.count)
            } finally {
                SchedulingCodecShadow.resume.countDown()
                worker.join(2_000)
            }
            assertTrue(SchedulingCodecShadow.released.await(2, TimeUnit.SECONDS))
        }
    }
    @Test fun t386_recreatedReceiverWaitsForRetiredNativeOwner() {
        Fixture().use { old ->
            old.blockedInput()
            old.decoder.releaseCodec()
            val surface = Surface(SurfaceTexture(2))
            val fresh = DecoderSession(Any(), { true }, FrameTiming(), object : DecoderEvents {
                override fun rendered(sequence: Int, decodeMicros: Int) {}
                override fun invalidated() {}
            }, { ReceiverStatistics() })
            var created = 0
            fresh.createCodec = { mime -> created++; MediaCodec.createDecoderByType(mime) }
            val format = DecoderFormat(VideoReceiver.MIME_TYPE, 1280, 800, 60)
            try {
                assertFalse("T386: recreated receiver bypassed outstanding retirement", fresh.setupCodec(surface, format))
                assertEquals("T386: blocked setup must not allocate a native codec", 0, created)
                SchedulingCodecShadow.resume.countDown()
                assertTrue(SchedulingCodecShadow.released.await(2, TimeUnit.SECONDS))
                val deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(2)
                while (!fresh.setupCodec(surface, format) && System.nanoTime() < deadline) Thread.sleep(10)
                assertNotNull("T386: completed retirement must reopen admission", fresh.mediaCodec)
                assertEquals(1, created)
            } finally {
                fresh.releaseCodec()
                surface.release()
                SchedulingCodecShadow.resume.countDown()
            }
        }
    }

}
