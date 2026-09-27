// Copyright (c) 2026 Geraldo Netto
package com.blent

import android.media.MediaCodec
import android.media.MediaCodecInfo
import android.media.MediaFormat
import android.os.Build
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.Implementation
import org.robolectric.annotation.Implements
import org.robolectric.shadows.ShadowMediaCodec
import org.robolectric.shadows.MediaCodecInfoBuilder
import org.robolectric.shadows.ShadowMediaCodecList

// API27's stock Robolectric codec shadow has no native identity implementation.
@Implements(MediaCodec::class)
class DiagnosticCodecShadow : ShadowMediaCodec() {
    companion object { var information: MediaCodecInfo? = null }
    @Implementation fun getName(): String = checkNotNull(information).name
    @Implementation fun getCodecInfo(): MediaCodecInfo = checkNotNull(information)
}

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34], shadows = [DiagnosticCodecShadow::class])
class DecoderDiagnosticsTest {
    private fun info(low: Boolean): MediaCodecInfo {
        val format = MediaFormat.createVideoFormat("video/avc", 1920, 1080)
        format.setFeatureEnabled("low-latency", low)
        val caps = MediaCodecInfoBuilder.CodecCapabilitiesBuilder.newBuilder()
            .setMediaFormat(format).setIsEncoder(false).setColorFormats(intArrayOf(19))
            .setProfileLevels(arrayOf(MediaCodecInfo.CodecProfileLevel().apply {
                profile = MediaCodecInfo.CodecProfileLevel.AVCProfileHigh
                level = MediaCodecInfo.CodecProfileLevel.AVCLevel52
            })).build()
        return MediaCodecInfoBuilder.newBuilder().setName("t417.decoder").setIsEncoder(false)
            .setIsHardwareAccelerated(true).setIsSoftwareOnly(false).setCapabilities(caps).build()
    }

    @Test fun t417_nativeCapabilitiesRemainSeparateFromRequestedHints() {
        for (supported in listOf(false, true)) {
            ShadowMediaCodecList.reset()
            DiagnosticCodecShadow.information = info(supported)
            ShadowMediaCodecList.addCodec(DiagnosticCodecShadow.information!!)
            val codec = MediaCodec.createByCodecName("t417.decoder")
            val parameters = DecoderFormat("video/avc", 640, 480, 30)
            try {
                val configured = DecoderConfiguration.format(codec, parameters, DecoderProfile(), true)
                val row = DecoderDiagnosticProbe.capture(codec, parameters, configured)
                assertEquals("t417.decoder", row.name)
                assertEquals(if (Build.VERSION.SDK_INT >= 29) true else null, row.hardware)
                assertEquals(if (Build.VERSION.SDK_INT >= 30) supported else null, row.lowLatency)
                assertEquals(true, row.doubleRate)
                assertTrue(row.requested.contains("Operating rate=60"))
                assertTrue(row.requested.contains("Vendor low latency=1"))
                assertEquals(Build.VERSION.SDK_INT >= 30, row.requested.contains("Standard low latency=1"))
                val unknown = DecoderDiagnosticProbe.capture(codec, parameters.copy(mimeType = "video/absent"), configured)
                assertNull(unknown.lowLatency)
                assertNull(unknown.doubleRate)
                assertEquals(false, DecoderDiagnosticProbe.capture(codec, parameters.copy(width = Int.MAX_VALUE), configured).doubleRate)
                val disabled = DecoderConfiguration.format(codec, parameters, DecoderProfile(), false)
                assertTrue(DecoderDiagnosticProbe.capture(codec, parameters, disabled).requested.isEmpty())
                DiagnosticCodecShadow.information = null
                val unavailable = DecoderDiagnosticProbe.capture(codec, parameters, configured)
                assertNull(unavailable.name)
                assertNull(unavailable.hardware)
                assertNull(unavailable.lowLatency)
                assertNull(unavailable.doubleRate)
            } finally { codec.release(); ShadowMediaCodecList.reset() }
        }
    }

    @Test fun t417_invalidRangesAndMalformedOptionalHintsRemainUnknown() {
        val caps = info(false).getCapabilitiesForType("video/avc")
        val base = DecoderFormat("video/avc", 640, 480, 30)
        for (value in listOf(Int.MIN_VALUE, -1, 0)) {
            assertNull(DecoderDiagnosticProbe.doubleRate(caps, base.copy(width = value)))
            assertNull(DecoderDiagnosticProbe.doubleRate(caps, base.copy(height = value)))
            assertNull(DecoderDiagnosticProbe.doubleRate(caps, base.copy(fps = value)))
        }
        for (fps in listOf(Int.MAX_VALUE / 2 + 1, Int.MAX_VALUE))
            assertNull(DecoderDiagnosticProbe.doubleRate(caps, base.copy(fps = fps)))
        for (fps in listOf(1, 30, 90, Int.MAX_VALUE / 2))
            assertNull(DecoderDiagnosticProbe.doubleRate(null, base.copy(fps = fps)))
        val malformed = MediaFormat().apply { setString("operating-rate", "not an integer") }
        assertEquals(listOf("Operating rate=unknown"), DecoderDiagnosticProbe.requested(malformed))
        assertEquals(listOf("No", "Yes", "Unknown"), listOf(false, true, null).map(::supportLabel))
    }
}
