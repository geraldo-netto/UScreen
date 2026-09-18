package com.uscreen.benchmark

import android.media.MediaCodecInfo
import android.media.MediaCodecList
import android.os.Build
import org.json.JSONArray
import org.json.JSONObject

/** T400: shell app_process entry point; queries capabilities without an Activity. */
object CodecInventory {
    private val types = setOf("video/avc", "video/hevc", "video/av01", "video/x-vnd.on2.vp9")
    @JvmStatic fun main(args: Array<String>) {
        val rows = JSONArray()
        MediaCodecList(MediaCodecList.REGULAR_CODECS).codecInfos.filter { !it.isEncoder }.forEach { info ->
            info.supportedTypes.filter { it in types }.forEach { mime ->
                rows.put(entry(info, mime))
            }
        }
        println("USCREEN_INVENTORY_V1:" + JSONObject().put("fingerprint", Build.FINGERPRINT)
            .put("sdk", Build.VERSION.SDK_INT).put("abis", JSONArray(Build.SUPPORTED_ABIS.toList())).put("decoders", rows))
    }
    private fun entry(info: MediaCodecInfo, mime: String): JSONObject {
        val row = JSONObject().put("name", info.name).put("mime", mime)
        if (Build.VERSION.SDK_INT >= 29) row.put("hardware", info.isHardwareAccelerated)
            .put("software", info.isSoftwareOnly).put("vendor", info.isVendor).put("alias", info.isAlias)
        try { capabilities(row, info.getCapabilitiesForType(mime)) }
        catch (error: Exception) { row.put("error", error.toString()) }
        return row
    }
    private fun capabilities(row: JSONObject, caps: MediaCodecInfo.CodecCapabilities) {
        row.put("max_instances_advertised", caps.maxSupportedInstances)
        row.put("profiles", JSONArray(caps.profileLevels.map { JSONObject().put("profile", it.profile).put("level", it.level) }))
        row.put("color_formats", JSONArray(caps.colorFormats.toList()))
        val features = mutableListOf(MediaCodecInfo.CodecCapabilities.FEATURE_AdaptivePlayback,
            MediaCodecInfo.CodecCapabilities.FEATURE_SecurePlayback, MediaCodecInfo.CodecCapabilities.FEATURE_TunneledPlayback)
        if (Build.VERSION.SDK_INT >= 30) features.add(MediaCodecInfo.CodecCapabilities.FEATURE_LowLatency)
        row.put("features", JSONArray(features.map { JSONObject().put("name", it)
            .put("supported", caps.isFeatureSupported(it)).put("required", caps.isFeatureRequired(it)) }))
        caps.videoCapabilities?.let { row.put("video", video(it)) }
    }
    private fun video(caps: MediaCodecInfo.VideoCapabilities): JSONObject = JSONObject()
        .put("widths", caps.supportedWidths.toString()).put("heights", caps.supportedHeights.toString())
        .put("frame_rates", caps.supportedFrameRates.toString()).put("bitrates", caps.bitrateRange.toString())
        .put("width_alignment", caps.widthAlignment).put("height_alignment", caps.heightAlignment)
        .put("modes", modes(caps))

    private fun modes(caps: MediaCodecInfo.VideoCapabilities): JSONArray = JSONArray().apply {
        for ((width, height) in listOf(1280 to 800, 1920 to 1080, 3840 to 2160)) {
            for (fps in listOf(30, 60, 90, 120)) {
                put(JSONObject().put("width", width).put("height", height).put("fps", fps)
                    .put("supported", caps.areSizeAndRateSupported(width, height, fps.toDouble())))
            }
        }
    }
}
