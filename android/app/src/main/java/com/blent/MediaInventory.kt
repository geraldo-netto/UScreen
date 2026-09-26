package com.blent

import android.media.MediaCodecInfo
import android.media.MediaFormat
import android.os.Build
import org.json.JSONArray
import org.json.JSONObject

/** Only positive, exact-format capabilities authorize a richer selection. */
internal object MediaInventory {
    fun describe(info: MediaCodecInfo, codec: String, width: Int, height: Int, fps: Int): JSONObject? {
        if (!identifier(info.name)) return null
        val row = JSONObject().put("name", info.name).put("codec", codec)
            .put("hardware", nullable { if (Build.VERSION.SDK_INT >= 29) info.isHardwareAccelerated else null })
            .put("low_latency", JSONObject.NULL).put("operating_rate", JSONObject.NULL)
            .put("profiles", JSONArray())
        try { capabilities(row, info.getCapabilitiesForType(VideoCodec.types.getValue(codec)), codec, width, height, fps) }
        catch (_: Exception) { /* Unknown optional detail never becomes a positive capability. */ }
        return row
    }

    private fun capabilities(row: JSONObject, caps: MediaCodecInfo.CodecCapabilities, codec: String, width: Int, height: Int, fps: Int) {
        row.put("low_latency", nullable {
            if (Build.VERSION.SDK_INT >= 30) caps.isFeatureSupported(MediaCodecInfo.CodecCapabilities.FEATURE_LowLatency) else null
        })
        row.put("operating_rate", nullable {
            (fps * 2).takeIf { caps.videoCapabilities?.areSizeAndRateSupported(width, height, it.toDouble()) == true }
        })
        val entries = caps.profileLevels.mapNotNull { pair ->
            profile(caps, codec, pair, width, height, fps)
        }.distinctBy { it.toString() }.take(32)
        row.put("profiles", JSONArray(entries))
    }

    private fun profile(caps: MediaCodecInfo.CodecCapabilities, codec: String,
                        pair: MediaCodecInfo.CodecProfileLevel, width: Int, height: Int, fps: Int): JSONObject? = try {
        val description = MediaProfiles.describe(codec, pair)
        val format = MediaFormat.createVideoFormat(VideoCodec.types.getValue(codec), width, height)
        format.setInteger(MediaFormat.KEY_FRAME_RATE, fps)
        format.setInteger(MediaFormat.KEY_PROFILE, pair.profile)
        description?.takeIf { caps.isFormatSupported(format) }
    } catch (_: Exception) { null }

    private fun nullable(query: () -> Any?): Any = try { query() ?: JSONObject.NULL } catch (_: Exception) { JSONObject.NULL }
    internal fun identifier(value: String): Boolean = value.isNotEmpty() && value.length <= 128 && value.all { it.code in 33..126 }
}
