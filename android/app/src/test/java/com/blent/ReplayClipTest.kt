package com.blent

import com.blent.benchmark.ReplayClip
import java.io.ByteArrayOutputStream
import java.io.DataOutputStream
import java.io.File
import org.junit.Assert.*
import org.junit.Test

/** T400: versioned benchmark fixtures preserve codec and complete access units. */
class ReplayClipTest {
    private fun bytes(version: Long, mime: String, config: ByteArray, frames: List<ByteArray>): ByteArray {
        val raw = ByteArrayOutputStream()
        DataOutputStream(raw).use { output ->
            output.writeLong(version)
            listOf(1280, 800, 60, frames.size).forEach(output::writeInt)
            if (version == 0x5553444230303032L) output.writeUTF(mime)
            listOf(config).plus(frames).forEach { output.writeInt(it.size); output.write(it) }
        }
        return raw.toByteArray()
    }

    private fun clip(bytes: ByteArray, check: (ReplayClip) -> Unit) {
        val file = File.createTempFile("blent-t400-", ".bin")
        try { file.writeBytes(bytes); check(ReplayClip(file)) } finally { file.delete() }
    }

    @Test fun t400_version_one_retains_h264_framing() {
        clip(bytes(0x5553444230303031L, "ignored", byteArrayOf(1, 2), listOf(byteArrayOf(3, 4)))) {
            assertEquals("video/avc", it.mime)
            assertArrayEquals(byteArrayOf(1, 2), it.config)
            assertArrayEquals(byteArrayOf(3, 4), it.frames.single())
            assertEquals(64, it.sha256.length)
        }
    }

    @Test fun t400_version_two_preserves_each_codec_and_optional_initialization() {
        for (mime in listOf("video/avc", "video/hevc", "video/x-vnd.on2.vp9", "video/av01")) {
            val config = if (mime.startsWith("video/av0") || mime.endsWith("vp9")) byteArrayOf() else byteArrayOf(1, 2)
            clip(bytes(0x5553444230303032L, mime, config, listOf(byteArrayOf(3)))) {
                assertEquals(mime, it.mime)
                assertArrayEquals(config, it.config)
                assertEquals(60, it.fps)
            }
        }
    }

    @Test fun t400_empty_coded_frames_missing_parameter_sets_and_unknown_mime_are_rejected() {
        for ((mime, config, frame) in listOf(Triple("video/hevc", byteArrayOf(), byteArrayOf(3)),
                Triple("video/av01", byteArrayOf(), byteArrayOf()), Triple("video/unknown", byteArrayOf(1), byteArrayOf(2)))) {
            assertThrows(IllegalArgumentException::class.java) {
                clip(bytes(0x5553444230303032L, mime, config, listOf(frame))) { fail("invalid fixture") }
            }
        }
    }

    @Test fun t400_truncated_oversized_payload_and_extra_bytes_are_rejected() {
        val valid = bytes(0x5553444230303032L, "video/hevc", byteArrayOf(1, 2), listOf(byteArrayOf(3, 4)))
        val oversized = valid.copyOf()
        // V2 header + modified-UTF MIME precedes the config length.
        java.nio.ByteBuffer.wrap(oversized).putInt(26 + "video/hevc".length, 8 * 1024 * 1024 + 1)
        assertThrows(IllegalArgumentException::class.java) { clip(oversized) { fail("oversized packet") } }
        assertThrows(java.io.EOFException::class.java) { clip(valid.copyOf(valid.size - 1)) { fail("truncated") } }
        assertThrows(IllegalArgumentException::class.java) { clip(valid + byteArrayOf(0)) { fail("extra bytes") } }
    }
}
