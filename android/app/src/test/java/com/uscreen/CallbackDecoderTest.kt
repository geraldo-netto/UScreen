package com.uscreen

import android.graphics.SurfaceTexture
import android.media.MediaCodec
import android.media.MediaCrypto
import android.media.MediaFormat
import android.os.Handler
import android.view.Surface
import java.nio.ByteBuffer
import java.util.concurrent.CountDownLatch
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit
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
class CallbackCodecShadow : ShadowMediaCodec() {
    data class Queued(val bytes: ByteArray, val sequence: Long, val flags: Int, val thread: String)
    companion object {
        val callbacks = LinkedBlockingQueue<MediaCodec.Callback>()
        val inputs = LinkedBlockingQueue<Queued>()
        val outputs = LinkedBlockingQueue<Int>()
        val rendered = LinkedBlockingQueue<MediaCodec.OnFrameRenderedListener>()
        val handlers = LinkedBlockingQueue<Handler>()
    }
    private val buffers = mutableMapOf<Int, ByteBuffer>()
    @Implementation fun configure(format: MediaFormat, surface: Surface?, crypto: MediaCrypto?, flags: Int) {}
    @Implementation fun setVideoScalingMode(mode: Int) {}
    @Implementation fun setCallback(callback: MediaCodec.Callback, handler: Handler) { callbacks.add(callback); handlers.add(handler) }
    @Implementation fun setOnFrameRenderedListener(listener: MediaCodec.OnFrameRenderedListener, handler: Handler) { rendered.add(listener) }
    @Implementation fun start() {}
    @Implementation fun stop() {}
    @Implementation fun release() {}
    @Implementation fun dequeueInputBuffer(timeoutUs: Long): Int = error("T386: synchronous input in callback mode")
    @Implementation fun dequeueOutputBuffer(info: MediaCodec.BufferInfo, timeoutUs: Long): Int = error("T386: synchronous output in callback mode")
    @Implementation fun getInputBuffer(index: Int): ByteBuffer = buffers.getOrPut(index) { ByteBuffer.allocate(1024) }
    @Implementation fun queueInputBuffer(index: Int, offset: Int, size: Int, presentationTimeUs: Long, flags: Int) {
        inputs.add(Queued(buffers[index]!!.array().copyOfRange(offset, offset + size), presentationTimeUs, flags, Thread.currentThread().name))
    }
    @Implementation override fun releaseOutputBuffer(index: Int, render: Boolean) { outputs.add(index) }
}

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34], shadows = [CallbackCodecShadow::class])
class CallbackDecoderTest {
    private class Fixture : AutoCloseable {
        private val surface = Surface(SurfaceTexture(1))
        val invalidated = CountDownLatch(1)
        val acknowledgements = LinkedBlockingQueue<Int>()
        val decoder = DecoderSession(Any(), { true }, FrameTiming(), object : DecoderEvents {
            override fun rendered(sequence: Int, decodeMicros: Int) { acknowledgements.add(sequence) }
            override fun invalidated() { invalidated.countDown() }
        }, { ReceiverStatistics() })
        init {
            CallbackCodecShadow.callbacks.clear(); CallbackCodecShadow.inputs.clear()
            CallbackCodecShadow.outputs.clear(); CallbackCodecShadow.rendered.clear()
            CallbackCodecShadow.handlers.clear()
            decoder.inputClock = { android.os.SystemClock.uptimeMillis() * 1_000_000 }
            decoder.profile = DecoderProfile(callbacks = true)
            start()
        }
        fun start() = assertTrue(decoder.setupCodec(surface, DecoderFormat(VideoReceiver.MIME_TYPE, 1280, 800, 60)))
        fun callback(): MediaCodec.Callback = CallbackCodecShadow.callbacks.poll(1, TimeUnit.SECONDS)!!
        fun feed(data: ByteArray, configuration: Boolean = false, sequence: Long = 9) {
            decoder.feedDecoder(decoder.mediaCodec!!, data, 0, data.size, configuration, sequence)
        }
        override fun close() { decoder.releaseCodec(); surface.release() }
    }

    @Test fun t386_callbackInputOwnsBytesAndPreservesConfigurationSequenceAndOrder() {
        Fixture().use { fixture ->
            val codec = fixture.decoder.mediaCodec!!
            val callback = fixture.callback()
            val config = byteArrayOf(1, 2)
            val frame = byteArrayOf(7, 8, 9)
            fixture.feed(config, true, 0)
            fixture.feed(frame, false, 0xffff_ffffL)
            config.fill(42); frame.fill(42)
            callback.onInputBufferAvailable(codec, 0)
            callback.onInputBufferAvailable(codec, 1)
            val first = CallbackCodecShadow.inputs.poll(1, TimeUnit.SECONDS)!!
            val second = CallbackCodecShadow.inputs.poll(1, TimeUnit.SECONDS)!!
            assertArrayEquals(byteArrayOf(1, 2), first.bytes)
            assertEquals(MediaCodec.BUFFER_FLAG_CODEC_CONFIG, first.flags)
            assertEquals(0, first.sequence)
            assertArrayEquals(byteArrayOf(7, 8, 9), second.bytes)
            assertEquals(0, second.flags)
            assertEquals(0xffff_ffffL, second.sequence)
            assertEquals("uscreen-frame-cb", first.thread)
            assertEquals(first.thread, second.thread)
        }
    }

    @Test fun t386_retiredCallbacksCannotQueueReleaseOrAcknowledgeNewCodec() {
        Fixture().use { fixture ->
            val old = fixture.decoder.mediaCodec!!
            val callback = fixture.callback()
            val oldRendered = CallbackCodecShadow.rendered.take()
            fixture.decoder.resetCodec()
            fixture.start()
            val current = fixture.decoder.mediaCodec!!
            val fresh = fixture.callback()
            fixture.feed(byteArrayOf(9))
            callback.onInputBufferAvailable(old, 9)
            callback.onOutputBufferAvailable(old, 9, MediaCodec.BufferInfo())
            oldRendered.onFrameRendered(old, 9, 0)
            fresh.onInputBufferAvailable(current, 0)
            assertArrayEquals(byteArrayOf(9), CallbackCodecShadow.inputs.poll(1, TimeUnit.SECONDS)!!.bytes)
            fresh.onOutputBufferAvailable(current, 2, MediaCodec.BufferInfo().apply { presentationTimeUs = 9 })
            assertEquals(2, CallbackCodecShadow.outputs.poll(1, TimeUnit.SECONDS))
            CallbackCodecShadow.rendered.take().onFrameRendered(current, 9, 0)
            assertEquals(9, fixture.acknowledgements.poll(1, TimeUnit.SECONDS))
            assertTrue(CallbackCodecShadow.outputs.isEmpty())
            assertTrue(fixture.acknowledgements.isEmpty())
        }
    }

    @Test fun t386_callbackInputDeadlineRetiresInsteadOfDroppingReferenceFrames() {
        Fixture().use { fixture ->
            fixture.feed(byteArrayOf(1))
            val scheduled = CountDownLatch(1)
            CallbackCodecShadow.handlers.take().post { scheduled.countDown() }
            assertTrue(scheduled.await(1, TimeUnit.SECONDS))
            org.robolectric.shadows.ShadowSystemClock.advanceBy(java.time.Duration.ofMillis(201))
            assertTrue(fixture.invalidated.await(2, TimeUnit.SECONDS))
            // Invalidation precedes destruction; acquire the owner guard by
            // calling release again before checking its final published state.
            fixture.decoder.releaseCodec()
            assertNull(fixture.decoder.mediaCodec)
            assertTrue(CallbackCodecShadow.inputs.isEmpty())
        }
    }

    @Test fun t386_mailboxBoundsAdmissionAndRetirementWakesBlockedProducer() {
        val waiting = CountDownLatch(1)
        var calls = 0
        val mailbox = DecoderMailbox {
            if (Thread.currentThread().name == "t386-producer" && ++calls == 2) waiting.countDown()
            1L
        }
        val input = DecoderInput(byteArrayOf(7), 0, 1, false, 9)
        assertTrue(mailbox.offer(input)); assertTrue(mailbox.offer(input))
        val result = LinkedBlockingQueue<Boolean>()
        val producer = Thread({ result.add(mailbox.offer(input)) }, "t386-producer").apply { start() }
        try {
            assertTrue(waiting.await(1, TimeUnit.SECONDS))
            assertTrue(result.isEmpty())
            mailbox.close()
            assertEquals(false, result.poll(1, TimeUnit.SECONDS))
            assertNull(mailbox.first())
            assertFalse(mailbox.offer(input))
        } finally { mailbox.close(); producer.join(1_000) }
    }
}
