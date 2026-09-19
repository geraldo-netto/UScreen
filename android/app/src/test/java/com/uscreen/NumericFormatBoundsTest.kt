package com.uscreen

import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/** T517: JSON numbers must survive validation without wrapping or truncation. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34])
class NumericFormatBoundsTest {
    private fun greeting() = JSONObject()
        .put("codec", "h264").put("video_width", 640).put("video_height", 480).put("fps", 60)

    private fun selection(): JSONObject = JSONObject(javaClass.getResource("/decoder-selection.json")!!.readText())

    @Test fun t517_streamNumbersRejectLossyCoercionBeforeDecoderSetup() {
        for (field in listOf("video_width", "video_height", "fps")) {
            val normal = greeting().getInt(field)
            for (value in listOf<Any>(normal.toLong() + (1L shl 32), normal - (1L shl 32),
                normal + 0.5, "$normal", JSONObject.NULL, Long.MIN_VALUE, Long.MAX_VALUE)) {
                val message = greeting().put(field, value)
                assertNull("T517 accepted $field=$value", StreamFormat.read(message, null, 1920, 1080, 60))
            }
        }
    }

    private fun mutatedSelection(field: String, value: Any): JSONObject = selection().also {
        val selected = it.getJSONObject("decoder_selection")
        when (field) {
            "decoder_protocol" -> it.put(field, value)
            "operating_rate" -> selected.put(field, value)
            else -> selected.getJSONObject("stream").put(field, value)
        }
    }

    @Test fun t517_decoderNumbersRejectLossyCoercionBeforeProfileAndHintValidation() {
        for ((field, normal) in listOf("decoder_protocol" to 2, "depth" to 8, "level" to 41, "operating_rate" to 120)) {
            for (value in listOf<Any>(normal.toLong() + (1L shl 32), normal - (1L shl 32),
                normal + 0.5, "$normal", Long.MIN_VALUE, Long.MAX_VALUE)) {
                val error = runCatching { DecoderSelection.read(mutatedSelection(field, value)) }.exceptionOrNull()
                assertTrue("T517 accepted $field=$value or failed unexpectedly: $error", error is IllegalArgumentException)
            }
        }
    }

    @Test fun t517_exactBoundariesAndLegacyDefaultsRemainUsable() {
        for (dimension in listOf(2, 4096)) {
            val message = greeting().put("video_width", dimension.toDouble()).put("video_height", dimension)
            assertEquals(dimension, StreamFormat.read(message, null, 0, 0, 60)!!.width)
        }
        for (fps in listOf(10, 90)) {
            assertEquals(fps, StreamFormat.read(greeting().put("fps", fps), null, 0, 0, 60)!!.fps)
        }
        val previous = DecoderFormat("video/hevc", 1280, 800, 30)
        assertEquals(previous, StreamFormat.read(JSONObject(), previous, 0, 0, 60))
        assertEquals(DecoderFormat("video/avc", 1920, 1080, 60), StreamFormat.read(JSONObject(), null, 0, 0, 60))
        for (rate in listOf(10, 180)) assertEquals(rate, DecoderSelection.read(mutatedSelection("operating_rate", rate))!!.operatingRate)
        assertNull(DecoderSelection.read(mutatedSelection("operating_rate", JSONObject.NULL))!!.operatingRate)
    }

    @Test fun t517_seededOutOfRangeJsonNumbersCannotReenterTheValidRange() {
        val random = java.util.Random(517)
        repeat(512) {
            val valid = random.nextInt(81) + 10
            val high = (random.nextInt(2047) + 1).toLong() shl 32
            for (number in listOf(high + valid, valid - high)) {
                assertNull(StreamFormat.read(greeting().put("fps", number), null, 1920, 1080, 60))
                assertThrows(IllegalArgumentException::class.java) {
                    DecoderSelection.read(mutatedSelection("operating_rate", number))
                }
            }
        }
    }
}
