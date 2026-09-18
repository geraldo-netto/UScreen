package com.uscreen

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
    private fun field(name: String) = VideoReceiver::class.java
        .getDeclaredField(name).apply { isAccessible = true }
    private fun packets(receiver: VideoReceiver, generation: Long): VideoPacketSink {
        val type = Class.forName("com.uscreen.VideoReceiver\$VideoPackets")
        return type.getDeclaredConstructor(VideoReceiver::class.java, Long::class.javaPrimitiveType)
            .apply { isAccessible = true }.newInstance(receiver, generation) as VideoPacketSink
    }
    @Suppress("UNCHECKED_CAST")
    @Test fun t432_wireConfigurationCreatesDecoderAtEncodedDimensionsAndRejectsRetiredRun() {
        StartupCodecShadow.stage = ""
        val receiver = VideoReceiver()
        receiver.mimeType = "video/x-vnd.on2.vp9"
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
        val configuration = ByteBuffer.allocate(13).putInt(0x55534331).put(3).putInt(640).putInt(400).array()
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
