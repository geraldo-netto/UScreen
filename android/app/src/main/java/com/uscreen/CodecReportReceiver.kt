package com.uscreen

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.media.MediaCodecInfo
import android.media.MediaCodecList
import android.os.Build

/** Read-only decoder inventory for `adb shell am broadcast`; DUMP restricts callers to shell/system. */
class CodecReportReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        resultData = decoderReport()
    }
}

internal fun decoderReport(
    codecs: () -> Array<MediaCodecInfo> = { MediaCodecList(MediaCodecList.REGULAR_CODECS).codecInfos },
): String {
    val prefix = "USCREEN_CODECS_V1:"
    return try {
        val entries = codecs().filter { !it.isEncoder && "video/hevc" in it.supportedTypes }
            .mapNotNull(::decoderEntry).distinct().sorted()
        prefix + if (entries.isEmpty()) "none" else entries.joinToString(",")
    } catch (_: Exception) {
        prefix + "unknown"
    }
}

private fun decoderEntry(codec: MediaCodecInfo): String? {
    val capabilities = codec.getCapabilitiesForType("video/hevc")
    if (capabilities.isFeatureRequired(MediaCodecInfo.CodecCapabilities.FEATURE_SecurePlayback) ||
        capabilities.isFeatureRequired(MediaCodecInfo.CodecCapabilities.FEATURE_TunneledPlayback)) {
        return null
    }
    val main10 = capabilities.profileLevels.any {
        it.profile == MediaCodecInfo.CodecProfileLevel.HEVCProfileMain10 ||
            it.profile == MediaCodecInfo.CodecProfileLevel.HEVCProfileMain10HDR10 ||
            it.profile == MediaCodecInfo.CodecProfileLevel.HEVCProfileMain10HDR10Plus
    }
    return decoderAcceleration(codec) + if (main10) "10" else "8"
}

private fun decoderAcceleration(codec: MediaCodecInfo): String = when {
    Build.VERSION.SDK_INT >= 29 && codec.isHardwareAccelerated -> "hw"
    Build.VERSION.SDK_INT >= 29 && codec.isSoftwareOnly -> "sw"
    codec.name.startsWith("OMX.google.") || codec.name.startsWith("c2.android.") -> "sw"
    else -> "unknown"
}
