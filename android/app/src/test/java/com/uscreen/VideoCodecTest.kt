package com.uscreen

import android.media.MediaCodec
import android.media.MediaFormat
import java.nio.ByteBuffer
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34])
class VideoCodecTest {
    @Test fun t497_configurationRecognitionAndBoundedMutationFuzz() {
        val random = java.util.Random(497)
        val framed = byteArrayOf(99) + header() + byteArrayOf(88)
        assertTrue(VideoCodec.hasConfigurationHeader(framed, 1, framed.size - 2))
        assertFalse(VideoCodec.hasConfigurationHeader(framed, 0, framed.size))
        for (size in 0..3) assertFalse(VideoCodec.hasConfigurationHeader(framed, 1, size))
        repeat(2048) {
            val data = header().copyOf(random.nextInt(70))
            if (data.isNotEmpty()) data[random.nextInt(data.size)] = random.nextInt().toByte()
            val parsed = runCatching { VideoCodec.configuration(data, 0, data.size, vp9, 60) }
            parsed.onSuccess {
                assertTrue(it.width in 2..4096)
                assertTrue(it.height in 2..4096)
                assertEquals(vp9, it.mimeType)
            }.onFailure { assertTrue("T497: unexpected parser failure: $it", it is IllegalArgumentException) }
        }
    }

    @Test fun t497_configurationRejectsOutOfBoundsSlices() {
        val data = header()
        for (offset in listOf(Int.MIN_VALUE, -1, 1, data.size, Int.MAX_VALUE)) {
            assertThrows(IndexOutOfBoundsException::class.java) {
                VideoCodec.configuration(data, offset, data.size, vp9, 60)
            }
        }
        for (size in listOf(Int.MIN_VALUE, -1, 0, 12, 65550, Int.MAX_VALUE)) {
            assertThrows(IllegalArgumentException::class.java) {
                VideoCodec.configuration(data, 0, size, vp9, 60)
            }
        }
    }

    private val vp9 = "video/x-vnd.on2.vp9"
    private fun header(width: Int = 640, private: ByteArray = byteArrayOf()): ByteArray =
        ByteBuffer.allocate(13 + private.size).putInt(0x55534331).put(3).putInt(width).putInt(400).put(private).array()

    @Test fun t433_av1ConfigurationAndCapabilitiesAreDistinctFromVp9() {
        val mime = "video/av01"
        assertEquals(mime, VideoCodec.types["av1"])
        assertTrue(VideoCodec.framed(mime))
        val data = header().apply { this[4] = 4 }
        val format = VideoCodec.configuration(data, 0, data.size, mime, 60)
        assertEquals(mime, format.mimeType)
        assertArrayEquals(byteArrayOf(), format.codecPrivate)
        val report = DecoderCapabilities.describe(640, 400, 60) { type, _, _, _ -> type == mime }
        assertEquals("[\"av1\"]", report.getJSONArray("codecs").toString())
        val oldPeer = DecoderCapabilities.describe(640, 400, 60) { type, _, _, _ -> type == "video/avc" }
        assertEquals("[\"h264\"]", oldPeer.getJSONArray("codecs").toString())
        assertThrows(IllegalArgumentException::class.java) {
            VideoCodec.configuration(header(), 0, 13, mime, 60)
        }
    }

    @Test fun t432_vp9ConfigurationIsMetadataNotAnEncodedAccessUnit() {
        val bytes = byteArrayOf(99) + header(private = byteArrayOf(1, 2)) + byteArrayOf(88)
        val parsed = VideoCodec.configuration(bytes, 1, bytes.size - 2, vp9, 60)
        assertEquals(vp9, parsed.mimeType)
        assertEquals(640, parsed.width)
        assertEquals(400, parsed.height)
        assertArrayEquals(byteArrayOf(1, 2), parsed.codecPrivate)
        val codec = MediaCodec.createDecoderByType(vp9)
        try {
            val format = DecoderConfiguration.format(codec, parsed, DecoderProfile(), false)
            assertEquals(640, format.getInteger(MediaFormat.KEY_WIDTH))
            assertEquals(ByteBuffer.wrap(byteArrayOf(1, 2)), format.getByteBuffer("csd-0"))
        } finally { codec.release() }
    }
    @Test fun t432_vp9MayHaveNoCodecPrivateData() {
        val parsed = VideoCodec.configuration(header(), 0, 13, vp9, 60)
        assertArrayEquals(byteArrayOf(), parsed.codecPrivate)
        val codec = MediaCodec.createDecoderByType(vp9)
        try {
            val format = DecoderConfiguration.format(codec, parsed, DecoderProfile(), false)
            assertFalse(format.containsKey("csd-0"))
        } finally { codec.release() }
    }
    @Test fun t432_invalidOrMismatchedConfigurationIsRejected() {
        for ((bytes, mime) in listOf(header() to "video/avc", header(0) to vp9,
            header(4097) to vp9, header().copyOf(12) to vp9, ByteArray(13) to vp9,
            header().apply { this[4] = 4 } to vp9)) {
            assertThrows(IllegalArgumentException::class.java) {
                VideoCodec.configuration(bytes, 0, bytes.size, mime, 60)
            }
        }
        assertNull(VideoCodec.types["unknown"])
    }
    @Test fun t432_capabilitiesDescribeExactGeometryAndOnlySupportedCodecs() {
        val report = DecoderCapabilities.describe(1280, 800, 60) { mime, width, height, fps ->
            assertEquals(Triple(1280, 800, 60), Triple(width, height, fps))
            mime == vp9
        }
        assertEquals(1, report.getInt("protocol"))
        assertEquals(1280, report.getInt("width"))
        assertEquals(800, report.getInt("height"))
        assertEquals(60, report.getInt("fps"))
        assertEquals("[\"vp9\"]", report.getJSONArray("codecs").toString())
    }
}
