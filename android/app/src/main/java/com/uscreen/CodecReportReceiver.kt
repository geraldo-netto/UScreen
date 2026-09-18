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
        resultData = decoderReport(intent.getIntExtra("uscreen_codecs_version", 1))
    }
}

internal fun decoderReport(
    version: Int = 1,
    codecs: () -> Array<MediaCodecInfo> = { MediaCodecList(MediaCodecList.REGULAR_CODECS).codecInfos },
): String {
    val inventory = try { codecs() } catch (_: Exception) { null }
    if (version == 2) {
        return "USCREEN_CODECS_V2:" + VideoCodec.types.entries.joinToString(";") { (name, mime) ->
            "$name=${decoderInventory(inventory, mime)}"
        }
    }
    // Preserve the shell API used by older hosts: V1 describes HEVC only.
    return "USCREEN_CODECS_V1:" + decoderInventory(inventory, "video/hevc")
}

private fun decoderInventory(codecs: Array<MediaCodecInfo>?, mime: String): String {
    if (codecs == null) return "unknown"
    return try {
        val entries = codecs.filter { !it.isEncoder && mime in it.supportedTypes }
            .mapNotNull { decoderEntry(it, mime) }.distinct().sorted()
        if (entries.isEmpty()) "none" else entries.joinToString(",")
    } catch (_: Exception) { "unknown" }
}

private fun decoderEntry(codec: MediaCodecInfo, mime: String): String? {
    val capabilities = codec.getCapabilitiesForType(mime)
    if (capabilities.isFeatureRequired(MediaCodecInfo.CodecCapabilities.FEATURE_SecurePlayback) ||
        capabilities.isFeatureRequired(MediaCodecInfo.CodecCapabilities.FEATURE_TunneledPlayback)) {
        return null
    }
    val acceleration = decoderAcceleration(codec)
    return if (mime == "video/hevc") acceleration + hevcDepth(capabilities) else if (acceleration == "unknown") "unclassified" else acceleration
}

private fun hevcDepth(capabilities: MediaCodecInfo.CodecCapabilities): String {
    val main10 = capabilities.profileLevels.any {
        it.profile == MediaCodecInfo.CodecProfileLevel.HEVCProfileMain10 ||
            it.profile == MediaCodecInfo.CodecProfileLevel.HEVCProfileMain10HDR10 ||
            it.profile == MediaCodecInfo.CodecProfileLevel.HEVCProfileMain10HDR10Plus
    }
    return if (main10) "10" else "8"
}

private fun decoderAcceleration(codec: MediaCodecInfo): String = when {
    Build.VERSION.SDK_INT >= 29 && codec.isHardwareAccelerated -> "hw"
    Build.VERSION.SDK_INT >= 29 && codec.isSoftwareOnly -> "sw"
    codec.name.startsWith("OMX.google.") || codec.name.startsWith("c2.android.") -> "sw"
    else -> "unknown"
}
