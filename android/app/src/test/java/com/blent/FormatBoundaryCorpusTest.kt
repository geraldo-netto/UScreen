package com.blent

import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/** T497: exercise rejected format/profile branches without allocating a decoder. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34])
class FormatBoundaryCorpusTest {
    private fun selection() = JSONObject(javaClass.getResource("/decoder-selection.json")!!.readText())

    @Test fun t497_numericBoundaryRejectsNonfiniteValuesFromAJsonProvider() {
        for (value in listOf(Double.NaN, Double.POSITIVE_INFINITY, Double.NEGATIVE_INFINITY)) {
            val source = object : JSONObject() { override fun get(name: String): Any = value }
            assertThrows(IllegalArgumentException::class.java) { JsonNumbers.integer(source, "fps", 10, 90) }
        }
    }

    @Test fun t497_unknownCodecsAndInvalidLegacyDefaultsCannotProduceAFormat() {
        assertNull(StreamFormat.read(JSONObject().put("codec", "unknown"), null, 1920, 1080, 60))
        assertNull(StreamFormat.read(selection().put("codec", "hevc"), null, 1920, 1080, 60))
        for (invalid in listOf(Int.MIN_VALUE, -1, 0, 1, 4097, Int.MAX_VALUE)) {
            assertNull(StreamFormat.read(JSONObject(), null, 640, invalid, 60))
            assertNull(StreamFormat.read(JSONObject(), DecoderFormat("video/avc", invalid, 480, 60), 0, 0, 60))
        }
        for (fps in listOf(Int.MIN_VALUE, -1, 0, 9, 91, Int.MAX_VALUE)) {
            assertNull(StreamFormat.read(JSONObject(), null, 1920, 1080, fps))
        }
        for (width in listOf(Int.MIN_VALUE, 0, 1, 4097, Int.MAX_VALUE)) {
            assertEquals(1920, StreamFormat.read(JSONObject(), null, width, 0, 60)!!.width)
        }
    }

    @Test fun t497_unknownProfileDepthAndCodecLevelsCannotBeSelected() {
        for (depth in listOf(8, 9, 10)) {
            val message = selection()
            message.getJSONObject("decoder_selection").getJSONObject("stream").put("profile", "unknown").put("depth", depth)
            assertThrows(IllegalArgumentException::class.java) { DecoderSelection.read(message) }
        }
        for (level in listOf(14, 19, 23, 33, 43, 53, 63, 70, 73)) {
            val message = selection()
            message.getJSONObject("decoder_selection").getJSONObject("stream").put("level", level)
            assertThrows(IllegalArgumentException::class.java) { DecoderSelection.read(message) }
        }
        val message = selection()
        message.getJSONObject("decoder_selection").getJSONObject("stream").put("codec", "unsupported")
        assertThrows(IllegalArgumentException::class.java) { DecoderSelection.read(message) }
    }

    @Test fun t497_profileLookupMutationCorpusNeverInventsAnAndroidProfile() {
        val names = listOf("baseline", "constrained-baseline", "main", "high", "main10", "profile0", "profile2", "unknown")
        for (codec in VideoCodec.types.keys + "unknown") {
            for (name in names) {
                for (depth in listOf(Int.MIN_VALUE, -1, 0, 8, 9, 10, Int.MAX_VALUE)) {
                    val selected = MediaProfiles.profile(codec, name, depth) ?: continue
                    assertEquals(name, selected.name)
                    assertEquals(depth, selected.depth)
                    assertEquals(selected, MediaProfiles.profile(codec, selected.android))
                }
            }
            for (value in listOf(Int.MIN_VALUE, -1, 0, Int.MAX_VALUE)) assertNull(MediaProfiles.profile(codec, value))
        }
    }
}
