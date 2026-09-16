package com.uscreen

import android.content.ComponentName
import android.media.MediaCodecInfo
import android.media.MediaFormat
import android.os.Build
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.shadows.MediaCodecInfoBuilder

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34])
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

    @Test fun t098_decoderInventoryDistinguishesHardwareSoftwareAbsentAndUnknown() {
        val hardware = codec("vendor.hevc", false, true)
        assertEquals("USCREEN_CODECS_V1:" + if (Build.VERSION.SDK_INT >= 29) "hw10" else "unknown10",
            decoderReport { arrayOf(hardware) })
        assertEquals("USCREEN_CODECS_V1:sw8", decoderReport { arrayOf(codec("OMX.google.hevc", true, false)) })
        assertEquals("USCREEN_CODECS_V1:none", decoderReport { arrayOf(codec("vendor.encoder", false, true, true)) })
        assertEquals("USCREEN_CODECS_V1:none", decoderReport { emptyArray() })
        assertEquals("USCREEN_CODECS_V1:unknown", decoderReport { throw IllegalStateException("unavailable") })
    }

    @Test fun t098_inventoryReceiverRequiresShellDumpPermission() {
        val context = RuntimeEnvironment.getApplication()
        val receiver = context.packageManager.getReceiverInfo(ComponentName(context, CodecReportReceiver::class.java), 0)
        assertTrue(receiver.exported)
        assertEquals("android.permission.DUMP", receiver.permission)
    }
}
