package com.uscreen

import android.content.ComponentName
import android.media.MediaCodec
import android.media.MediaCodecInfo
import android.media.MediaFormat
import android.os.Build
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.annotation.Implementation
import org.robolectric.annotation.Implements
import org.robolectric.shadows.ShadowMediaCodec
import org.robolectric.shadows.MediaCodecInfoBuilder
import org.robolectric.shadows.ShadowMediaCodecList
import kotlinx.coroutines.runBlocking

@Implements(MediaCodec::class)
class SelectionCodecShadow : ShadowMediaCodec() {
    companion object { var failCreation = false }
    private var requestedName = ""
    @Implementation public override fun __constructor__(name: String, nameIsType: Boolean, encoder: Boolean) {
        requestedName = name
        check(!failCreation) { "T468 injected creation failure" }
        super.__constructor__(name, nameIsType, encoder)
    }
    @Implementation fun getName() = requestedName
}

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34], shadows = [SelectionCodecShadow::class])
class CodecReportTest {
    private fun codec(name: String, software: Boolean, main10: Boolean, encoder: Boolean = false): MediaCodecInfo {
        val profile = MediaCodecInfo.CodecProfileLevel().apply {
            this.profile = if (main10) MediaCodecInfo.CodecProfileLevel.HEVCProfileMain10 else MediaCodecInfo.CodecProfileLevel.HEVCProfileMain
            level = MediaCodecInfo.CodecProfileLevel.HEVCMainTierLevel4
        }
        val capabilities = MediaCodecInfoBuilder.CodecCapabilitiesBuilder.newBuilder()
            .setMediaFormat(MediaFormat.createVideoFormat("video/hevc", 1920, 1080))
            .setIsEncoder(encoder).setProfileLevels(arrayOf(profile)).setColorFormats(intArrayOf(19)).build()
        return MediaCodecInfoBuilder.newBuilder().setName(name).setIsEncoder(encoder)
            .setIsSoftwareOnly(software).setIsHardwareAccelerated(!software)
            .setCapabilities(capabilities).build()
    }

    @Test fun t468_legacyDecoderCreationUsesTheAdvertisedCompatibleSelection() = runBlocking {
        ShadowMediaCodecList.reset()
        try {
            ShadowMediaCodecList.addCodec(codec("OMX.google.hevc", true, false))
            ShadowMediaCodecList.addCodec(codec("vendor.hevc", false, false))
            val parameters = DecoderFormat("video/hevc", 640, 480, 30)
            val report = DecoderCapabilities.report(parameters.width, parameters.height, parameters.fps)
            assertEquals("[\"hevc\"]", report.getJSONArray("codecs").toString())
            val expected = if (Build.VERSION.SDK_INT >= 29) "vendor.hevc" else "OMX.google.hevc"
            val decoder = DecoderConfiguration.create(parameters)
            try { assertEquals("T468: opened decoder differs from advertised selection", expected, decoder.name) }
            finally { decoder.release() }
        } finally { ShadowMediaCodecList.reset() }
    }

    @Test fun t468_creationFailureNamesTheChosenDecoderWithoutSilentFallback() {
        ShadowMediaCodecList.reset()
        try {
            ShadowMediaCodecList.addCodec(codec("vendor.hevc", false, false))
            SelectionCodecShadow.failCreation = true
            val error = runCatching { DecoderConfiguration.create(DecoderFormat("video/hevc", 640, 480, 30)) }.exceptionOrNull()
            assertTrue("T468: chosen decoder missing from failure: $error", error?.message?.contains("vendor.hevc") == true)
            assertNotNull(error?.cause)
        } finally { SelectionCodecShadow.failCreation = false; ShadowMediaCodecList.reset() }
    }

    @Test fun t468_unsupportedLegacyFormatNeverAllocatesTheDefaultDecoder() {
        ShadowMediaCodecList.reset()
        try {
            ShadowMediaCodecList.addCodec(codec("vendor.hevc", false, false))
            val invalid = DecoderFormat("video/hevc", 4096, 4096, 90)
            val error = runCatching { DecoderConfiguration.create(invalid).release() }.exceptionOrNull()
            assertTrue("T468: unsupported format used the default codec: $error", error is IllegalStateException)
            ShadowMediaCodecList.reset()
            assertTrue("T468: missing decoder accepted", runCatching {
                DecoderConfiguration.create(DecoderFormat("video/avc", 640, 480, 30)).release()
            }.isFailure)
        } finally { ShadowMediaCodecList.reset() }
    }

    @Test fun t098_decoderInventoryDistinguishesHardwareSoftwareAbsentAndUnknown() {
        val hardware = codec("vendor.hevc", false, true)
        assertEquals("USCREEN_CODECS_V1:" + if (Build.VERSION.SDK_INT >= 29) "hw10" else "unknown10",
            decoderReport { arrayOf(hardware) })
        assertEquals("USCREEN_CODECS_V1:sw8", decoderReport { arrayOf(codec("OMX.google.hevc", true, false)) })
        assertEquals("USCREEN_CODECS_V1:none", decoderReport { arrayOf(codec("vendor.encoder", false, true, true)) })
        assertEquals("USCREEN_CODECS_V1:none", decoderReport { emptyArray() })
        assertEquals("USCREEN_CODECS_V1:unknown", decoderReport { throw IllegalStateException("unavailable") })
    }

    @Test fun t438_version_two_inventory_distinguishes_codec_families() {
        val hardware = codec("vendor.hevc", false, true)
        val report = decoderReport(version = 2) { arrayOf(hardware) }
        val acceleration = if (Build.VERSION.SDK_INT >= 29) "hw10" else "unknown10"
        assertEquals("USCREEN_CODECS_V2:h264=none;hevc=$acceleration;vp9=none;av1=none", report)
        assertEquals("USCREEN_CODECS_V2:h264=unknown;hevc=unknown;vp9=unknown;av1=unknown",
            decoderReport(version = 2) { throw IllegalStateException("unavailable") })
    }

    @Test fun t098_inventoryReceiverRequiresShellDumpPermission() {
        val context = RuntimeEnvironment.getApplication()
        val receiver = context.packageManager.getReceiverInfo(ComponentName(context, CodecReportReceiver::class.java), 0)
        assertTrue(receiver.exported)
        assertEquals("android.permission.DUMP", receiver.permission)
    }

    @Test fun t434_empty_decoder_inventory_never_advertises_support() = runBlocking {
        ShadowMediaCodecList.reset()
        val report = DecoderCapabilities.report(640, 480, 30)
        assertEquals(0, report.getJSONArray("codecs").length())
        assertEquals(0, report.getJSONArray("hardware").length())
    }

    @Test fun t434_negotiation_reports_supported_hardware_without_guessing_on_old_android() = runBlocking {
        ShadowMediaCodecList.reset()
        try {
            ShadowMediaCodecList.addCodec(codec("OMX.google.hevc", true, false))
            ShadowMediaCodecList.addCodec(codec("vendor.hevc", false, false))
            ShadowMediaCodecList.addCodec(codec("vendor.encoder", false, true, true))
            val report = DecoderCapabilities.report(640, 480, 30)
            assertEquals("[\"hevc\"]", report.getJSONArray("codecs").toString())
            assertEquals(if (Build.VERSION.SDK_INT >= 29) "[\"hevc\"]" else "[]",
                report.getJSONArray("hardware").toString())
            val format = MediaFormat.createVideoFormat("video/hevc", 640, 480)
            format.setInteger(MediaFormat.KEY_FRAME_RATE, 30)
            assertEquals(if (Build.VERSION.SDK_INT >= 29) "vendor.hevc" else "OMX.google.hevc",
                DecoderCapabilities.decoderName(format, "video/hevc"))
            assertNull(DecoderCapabilities.decoderName(MediaFormat.createVideoFormat("video/av01", 640, 480), "video/av01"))
        } finally { ShadowMediaCodecList.reset() }
    }
}
