package com.uscreen

import android.graphics.SurfaceTexture
import android.media.MediaCodec
import android.media.MediaCrypto
import android.media.MediaFormat
import android.os.Handler
import android.view.Surface
import java.io.EOFException
import java.nio.ByteBuffer
import java.util.concurrent.CountDownLatch
import java.util.concurrent.FutureTask
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
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
class DirectInputCodecShadow : ShadowMediaCodec() {
    data class Queued(val bytes: ByteArray, val sequence: Long, val flags: Int)
    companion object {
        val input: ByteBuffer = ByteBuffer.allocateDirect(64)
        val queued = LinkedBlockingQueue<Queued>()
        val released = AtomicInteger()
        var beforeInput: () -> Unit = {}
    }
    @Implementation fun configure(format: MediaFormat, surface: Surface?, crypto: MediaCrypto?, flags: Int) {}
    @Implementation fun setVideoScalingMode(mode: Int) {}
    @Implementation fun setOnFrameRenderedListener(listener: MediaCodec.OnFrameRenderedListener, handler: Handler) {}
    @Implementation fun setCallback(callback: MediaCodec.Callback, handler: Handler) {}
    @Implementation fun start() {}
    @Implementation fun stop() {}
    @Implementation fun release() { released.incrementAndGet() }
    @Implementation fun dequeueInputBuffer(timeoutUs: Long): Int { beforeInput(); return 0 }
    @Implementation fun getInputBuffer(index: Int): ByteBuffer = input
    @Implementation fun queueInputBuffer(index: Int, offset: Int, size: Int, presentationTimeUs: Long, flags: Int) {
        val bytes = ByteArray(size)
        input.duplicate().apply { position(offset); limit(offset + size); get(bytes) }
        queued.add(Queued(bytes, presentationTimeUs, flags))
    }
    @Implementation fun dequeueOutputBuffer(info: MediaCodec.BufferInfo, timeoutUs: Long): Int {
        Thread.sleep(10)
        return -1
    }
}

internal class DirectInputFixture(callbacks: Boolean = false) : AutoCloseable {
    val monitor = Any()
    private val surface = Surface(SurfaceTexture(1))
    var closeTransport: () -> Unit = {}
    val invalidations = AtomicInteger()
    val decoder = DecoderSession(monitor, { true }, FrameTiming(), object : DecoderEvents {
        override fun rendered(sequence: Int, decodeMicros: Int) {}
        override fun invalidated() { invalidations.incrementAndGet(); closeTransport() }
    }, { ReceiverStatistics() })
    val codec: MediaCodec get() = decoder.mediaCodec!!
    init {
        DirectInputCodecShadow.queued.clear()
        DirectInputCodecShadow.released.set(0)
        DirectInputCodecShadow.beforeInput = {}
        decoder.profile = DecoderProfile(callbacks = callbacks)
        assertTrue(decoder.setupCodec(surface, DecoderFormat(VideoReceiver.MIME_TYPE, 1280, 800, 60)))
    }
    override fun close() { closeTransport(); decoder.releaseCodec(); surface.release() }
}

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34], shadows = [DirectInputCodecShadow::class])
class DirectDecoderInputTest {

    @Test fun t403_fill_uses_codec_owned_storage_and_preserves_csd_and_unsigned_sequence() {
        DirectInputFixture().use { fixture ->
            assertTrue(fixture.decoder.feedDirect(fixture.codec, ChannelPacketHeader(2, true, 0)) {
                assertSame(DirectInputCodecShadow.input, it)
                assertTrue(it.isDirect)
                it.put(byteArrayOf(1, 2))
            })
            assertTrue(fixture.decoder.feedDirect(fixture.codec, ChannelPacketHeader(3, false, 0xffff_ffffL)) {
                it.put(byteArrayOf(7, 8, 9))
            })
            val config = DirectInputCodecShadow.queued.remove()
            val frame = DirectInputCodecShadow.queued.remove()
            assertArrayEquals(byteArrayOf(1, 2), config.bytes)
            assertEquals(MediaCodec.BUFFER_FLAG_CODEC_CONFIG, config.flags)
            assertEquals(0, config.sequence)
            assertArrayEquals(byteArrayOf(7, 8, 9), frame.bytes)
            assertEquals(0, frame.flags)
            assertEquals(0xffff_ffffL, frame.sequence)
        }
    }

    @Test fun t403_partial_eof_never_queues_a_codec_sample() {
        DirectInputFixture().use { fixture ->
            assertFalse(fixture.decoder.feedDirect(fixture.codec, ChannelPacketHeader(4, false, 1)) {
                it.put(7); throw EOFException("partial payload")
            })
            assertTrue(DirectInputCodecShadow.queued.isEmpty())
            assertNull(fixture.decoder.mediaCodec)
            assertEquals(1, fixture.invalidations.get())
        }
    }

    @Test fun t403_short_fill_cannot_queue_stale_codec_slot_bytes() {
        DirectInputFixture().use { fixture ->
            assertFalse(fixture.decoder.feedDirect(fixture.codec, ChannelPacketHeader(4, false, 1)) { it.put(7) })
            assertTrue(DirectInputCodecShadow.queued.isEmpty())
            assertNull(fixture.decoder.mediaCodec)
        }
    }

    @Test fun t403_capacity_is_checked_before_transport_reads() {
        DirectInputFixture().use { fixture ->
            assertFalse(fixture.decoder.feedDirect(fixture.codec, ChannelPacketHeader(65, false, 1)) {
                fail("T403: undersized codec slot must not consume payload")
            })
            assertTrue(DirectInputCodecShadow.queued.isEmpty())
            assertEquals(1, fixture.invalidations.get())
        }
    }

    @Test fun t403_callback_profile_never_uses_synchronous_direct_input() {
        DirectInputFixture(callbacks = true).use { fixture ->
            assertFalse(fixture.decoder.feedDirect(fixture.codec, ChannelPacketHeader(1, false, 1)) { fail("must not fill") })
            assertTrue(DirectInputCodecShadow.queued.isEmpty())
            assertNotNull(fixture.decoder.mediaCodec)
        }
    }

    @Test fun t403_blocked_channel_read_is_cancelled_without_holding_receiver_monitor() {
        DirectInputFixture().use { fixture ->
            PacketSocketPair().use { sockets ->
                ChannelPacketReader(sockets.client).use { reader ->
                    sockets.send(byteArrayOf(0, 0, 0, 9, 1, 0, 0, 0, 2))
                    val header = reader.readHeader()
                    fixture.closeTransport = reader::close
                    val entered = CountDownLatch(1)
                    val codec = fixture.codec
                    val feed = FutureTask { fixture.decoder.feedDirect(codec, header) { entered.countDown(); reader.readPayload(it) } }
                    Thread(feed).start()
                    assertTrue(entered.await(1, TimeUnit.SECONDS))
                    val stopped = FutureTask { synchronized(fixture.monitor) { fixture.decoder.resetCodec() } }
                    Thread(stopped).start()
                    stopped.get(750, TimeUnit.MILLISECONDS)
                    assertFalse(feed.get(750, TimeUnit.MILLISECONDS))
                    assertTrue(DirectInputCodecShadow.queued.isEmpty())
                    assertEquals(1, DirectInputCodecShadow.released.get())
                }
            }
        }
    }

    @Test fun t403_late_fill_cannot_queue_after_native_owner_retirement() {
        DirectInputFixture().use { fixture ->
            val entered = CountDownLatch(1)
            val resume = CountDownLatch(1)
            val codec = fixture.codec
            val feed = FutureTask {
                fixture.decoder.feedDirect(codec, ChannelPacketHeader(1, false, 1)) {
                    entered.countDown()
                    check(resume.await(3, TimeUnit.SECONDS))
                    it.put(7)
                }
            }
            Thread(feed).start()
            assertTrue(entered.await(1, TimeUnit.SECONDS))
            try {
                fixture.decoder.releaseCodec()
                assertEquals(0, DirectInputCodecShadow.released.get())
            } finally { resume.countDown() }
            assertFalse(feed.get(750, TimeUnit.MILLISECONDS))
            assertTrue(DirectInputCodecShadow.queued.isEmpty())
        }
    }
}
