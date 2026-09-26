package com.blent

import android.graphics.SurfaceTexture
import android.media.MediaCodec
import android.view.Surface
import java.nio.ByteBuffer
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicReference
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34], shadows = [StartupCodecShadow::class])
class FramedDecoderTest {
    @Suppress("UNCHECKED_CAST")
    @Test fun t627_surfaceSetupPreservesNegotiatedDecoderAndReceiptAcrossReconnect() {
        StartupCodecShadow.stage = ""
        val receiver = VideoReceiver { error("T627 must not connect") }
        val surface = Surface(SurfaceTexture(1))
        val choice = DecoderSelection("c2.unisoc.avc.decoder", "h264", "constrained-baseline", 40, 8, false, 120)
        val method = VideoReceiver::class.java.getDeclaredMethod("ensureSurfaceCodec", Long::class.javaPrimitiveType)
            .apply { isAccessible = true }
        val attempted = mutableListOf<DecoderFormat>()
        receiver.decoder.createCodec = { attempted.add(it); MediaCodec.createDecoderByType(it.mimeType) }
        try {
            for (selected in listOf(choice, choice, null)) {
                receiver.setStreamFormat(DecoderFormat("video/avc", 1280, 800, 60, selection = selected))
                (field("pendingSurface").get(receiver) as AtomicReference<Surface?>).set(surface)
                (field("surfaceReady").get(receiver) as AtomicBoolean).set(true)
                field("isRunning").set(receiver, true)
                val generation = (field("sessionGeneration").get(receiver) as java.util.concurrent.atomic.AtomicLong).get()
                assertEquals(true, method.invoke(receiver, generation))
                assertEquals("T627: live surface setup discarded the negotiated decoder", selected, attempted.last().selection)
                assertEquals(selected?.receipt(), receiver.decoder.configuredSelectionReceipt)
                receiver.stop()
            }
            assertEquals(3, attempted.size)
        } finally { receiver.stop(); surface.release() }
    }

    @Suppress("UNCHECKED_CAST")
    @Test fun t497_surfaceCodecRequiresCurrentReadySurfaceAndReusesItsOwner() {
        StartupCodecShadow.stage = ""
        val receiver = VideoReceiver { error("T497 must not connect") }
        val surface = Surface(SurfaceTexture(1))
        val pending = field("pendingSurface").get(receiver) as AtomicReference<Surface?>
        val ready = field("surfaceReady").get(receiver) as AtomicBoolean
        val method = VideoReceiver::class.java.getDeclaredMethod("ensureSurfaceCodec", Long::class.javaPrimitiveType)
            .apply { isAccessible = true }
        var allocations = 0
        receiver.decoder.createCodec = { allocations++; MediaCodec.createDecoderByType(it.mimeType) }
        try {
            assertEquals(false, method.invoke(receiver, 0L))
            field("isRunning").set(receiver, true)
            assertEquals(false, method.invoke(receiver, 0L))
            pending.set(surface)
            assertEquals(false, method.invoke(receiver, 0L))
            assertEquals(0, allocations)
            ready.set(true)
            assertEquals(true, method.invoke(receiver, 0L))
            val owner = receiver.decoder.mediaCodec
            assertEquals(true, method.invoke(receiver, 0L))
            assertSame(owner, receiver.decoder.mediaCodec)
            assertEquals(1, allocations)
            receiver.stop()
            assertEquals(false, method.invoke(receiver, 0L))
        } finally { receiver.stop(); surface.release() }
    }

    @Suppress("UNCHECKED_CAST")
    @Test fun t497_framedSetupRejectsRetiredSurfaceWrongDimensionsAndFailedAllocation() {
        val receiver = VideoReceiver { error("T497 must not connect") }
        receiver.mimeType = "video/x-vnd.on2.vp9"
        val surface = Surface(SurfaceTexture(1))
        val configuration = ByteBuffer.allocate(13).putInt(0x424c4e31).put(3).putInt(640).putInt(400).array()
        field("isRunning").set(receiver, true)
        var allocations = 0
        receiver.decoder.createCodec = { allocations++; throw IllegalStateException("T497 refused codec") }
        try {
            assertThrows(IllegalStateException::class.java) { packets(receiver, 0).configuration(configuration, 0, 13) }
            (field("pendingSurface").get(receiver) as AtomicReference<Surface?>).set(surface)
            (field("surfaceReady").get(receiver) as AtomicBoolean).set(true)
            field("decoderSelection").set(receiver, DecoderSelection("fixture", "vp9", "profile0", 31, 8, false, null))
            assertThrows(IllegalArgumentException::class.java) { packets(receiver, 0).configuration(configuration, 0, 13) }
            assertEquals(0, allocations)
            field("decoderSelection").set(receiver, null)
            assertThrows(IllegalStateException::class.java) { packets(receiver, 0).configuration(configuration, 0, 13) }
            assertEquals(1, allocations)
            assertNull(receiver.decoder.mediaCodec)
        } finally { receiver.stop(); surface.release() }
    }

    private fun field(name: String) = VideoReceiver::class.java
        .getDeclaredField(name).apply { isAccessible = true }
    private fun packets(receiver: VideoReceiver, generation: Long): VideoPacketSink {
        val type = Class.forName("com.blent.VideoReceiver\$VideoPackets")
        return type.getDeclaredConstructor(VideoReceiver::class.java, Long::class.javaPrimitiveType)
            .apply { isAccessible = true }.newInstance(receiver, generation) as VideoPacketSink
    }
    @Test fun t432_wireConfigurationCreatesDecoderAtEncodedDimensionsAndRejectsRetiredRun() {
        configurationCreatesDecoder("video/x-vnd.on2.vp9", 3)
    }
    @Test fun t433_wireConfigurationCreatesAv1DecoderAtEncodedDimensionsAndRejectsRetiredRun() {
        configurationCreatesDecoder("video/av01", 4)
    }
    @Suppress("UNCHECKED_CAST")
    private fun configurationCreatesDecoder(mime: String, id: Byte) {
        StartupCodecShadow.stage = ""
        val receiver = VideoReceiver()
        receiver.mimeType = mime
        receiver.formatWidth = 1280
        receiver.formatHeight = 800
        val surface = Surface(SurfaceTexture(1))
        (field("pendingSurface").get(receiver) as AtomicReference<Surface>).set(surface)
        (field("surfaceReady").get(receiver) as AtomicBoolean).set(true)
        field("isRunning").set(receiver, true)
        val formats = mutableListOf<DecoderFormat>()
        receiver.decoder.createCodec = { parameters ->
            formats.add(parameters)
            MediaCodec.createDecoderByType(parameters.mimeType)
        }
        val configuration = ByteBuffer.allocate(13).putInt(0x424c4e31).put(id).putInt(640).putInt(400).array()
        try {
            assertThrows(IllegalStateException::class.java) { packets(receiver, 1).configuration(configuration, 0, 13) }
            assertTrue(formats.isEmpty())
            packets(receiver, 0).configuration(configuration, 0, 13)
            assertEquals(1, formats.size)
            assertEquals(640, formats.single().width)
            assertEquals(400, formats.single().height)
            assertArrayEquals(byteArrayOf(), formats.single().codecPrivate)
            assertNotNull(receiver.decoder.mediaCodec)
        } finally { receiver.stop(); surface.release() }
    }
}
