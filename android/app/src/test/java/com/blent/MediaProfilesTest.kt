package com.blent

import android.media.MediaCodecInfo.CodecProfileLevel as Levels
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34])
class MediaProfilesTest {
    @Test fun t484_decoderReceiptMatchesSharedVectorAndOmitsDisabledHints() {
        val vector = JSONObject(javaClass.getResource("/decoder-selection.json")!!.readText())
        val selection = DecoderSelection.read(vector)!!
        assertEquals(vector.getString("receipt"), selection.receipt())
        assertEquals("10:vendor.avc:h264:baseline:41:8:0:0", selection.receipt(false))
        assertNotEquals(selection.receipt(), selection.copy(name = "old.decoder").receipt())
    }

    @Test fun t478_standardLevelTranslationRejectsUnknownValuesAndHighTier() {
        assertEquals(41, MediaProfiles.level("h264", Levels.AVCLevel41))
        assertEquals(9, MediaProfiles.level("h264", Levels.AVCLevel1b))
        assertEquals(40, MediaProfiles.level("hevc", Levels.HEVCMainTierLevel4))
        assertNull(MediaProfiles.level("hevc", Levels.HEVCHighTierLevel4))
        assertEquals(51, MediaProfiles.level("vp9", Levels.VP9Level51))
        assertEquals(31, MediaProfiles.level("av1", Levels.AV1Level31))
        assertNull(MediaProfiles.level("h264", 3))
        assertNull(MediaProfiles.profile("h264", 123456))
    }

    @Test fun t478_sharedProfileVectorAndMalformedSelectionRemainBounded() {
        val report = JSONObject(javaClass.getResource("/decoder-capabilities-v2.json")!!.readText())
        val profile = report.getJSONArray("details").getJSONObject(0).getJSONArray("profiles").getJSONObject(0)
        assertNotNull(MediaProfiles.profile("h264", profile.getString("profile"), profile.getInt("depth")))
        val selected = JSONObject().put("name", "vendor.avc").put("stream", JSONObject(profile.toString()).put("codec", "h264"))
            .put("low_latency", false).put("operating_rate", 120)
        val message = JSONObject().put("decoder_protocol", 2).put("decoder_selection", selected)
        assertEquals(41, DecoderSelection.read(message)!!.level)
        selected.put("name", "x".repeat(129))
        assertTrue(runCatching { DecoderSelection.read(message) }.isFailure)
        selected.put("name", "vendor.avc").put("operating_rate", 181)
        assertTrue(runCatching { DecoderSelection.read(message) }.isFailure)
        message.put("decoder_protocol", 3)
        assertTrue(runCatching { DecoderSelection.read(message) }.isFailure)
    }
}
