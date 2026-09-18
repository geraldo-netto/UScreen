package com.uscreen

import android.media.MediaCodecInfo
import android.media.MediaCodecList
import android.media.MediaFormat
import org.json.JSONObject

/** A request, not evidence that Android honored a hint or rendered a frame. */
internal data class DecoderSelection(
    val name: String, val codec: String, val profile: String, val level: Int, val depth: Int,
    val lowLatency: Boolean, val operatingRate: Int?,
) {
    fun stream(): JSONObject = JSONObject().put("codec", codec).put("profile", profile).put("level", level).put("depth", depth)

    fun validate(parameters: DecoderFormat): String {
        require(VideoCodec.types[codec] == parameters.mimeType) { "Decoder selection codec mismatch" }
        val info = MediaCodecList(MediaCodecList.REGULAR_CODECS).codecInfos.firstOrNull { !it.isEncoder && it.name == name }
        requireNotNull(info) { "Selected decoder is unavailable" }
        val report = MediaInventory.describe(info, codec, parameters.width, parameters.height, parameters.fps)
        requireNotNull(report) { "Selected decoder inventory unavailable" }
        val profiles = report.getJSONArray("profiles")
        require((0 until profiles.length()).any { MediaProfiles.covers(profiles.getJSONObject(it), stream()) }) {
            "Selected decoder does not support stream profile/level/depth"
        }
        validateHints(report)
        return name
    }

    private fun validateHints(report: JSONObject) {
        require(!lowLatency || report.optBoolean("low_latency")) { "Unsupported low latency hint" }
        require(operatingRate == null || report.optInt("operating_rate") >= operatingRate) { "Unsupported operating rate hint" }
    }

    fun configure(format: MediaFormat, allowHints: Boolean) {
        format.setInteger(MediaFormat.KEY_PROFILE, checkNotNull(MediaProfiles.profile(codec, profile, depth)).android)
        if (!allowHints) return
        if (lowLatency && android.os.Build.VERSION.SDK_INT >= 30) format.setInteger(MediaFormat.KEY_LOW_LATENCY, 1)
        operatingRate?.let { format.setInteger(MediaFormat.KEY_OPERATING_RATE, it) }
    }

    companion object {
        fun read(message: JSONObject): DecoderSelection? {
            if (message.isNull("decoder_selection")) return null
            require(message.optInt("decoder_protocol") == 2) { "Unsupported decoder selection version" }
            val selection = message.getJSONObject("decoder_selection")
            val stream = selection.getJSONObject("stream")
            val name = selection.getString("name")
            require(MediaInventory.identifier(name)) { "Invalid decoder identity" }
            val codec = stream.getString("codec")
            val profile = stream.getString("profile")
            val depth = stream.getInt("depth")
            requireNotNull(MediaProfiles.profile(codec, profile, depth)) { "Unknown stream profile" }
            val level = stream.getInt("level")
            require(MediaProfiles.validLevel(codec, level)) { "Invalid stream level" }
            val rate = if (selection.isNull("operating_rate")) null else selection.getInt("operating_rate")
            require(rate == null || rate in 10..180) { "Invalid operating rate" }
            return DecoderSelection(name, codec, profile, level, depth, selection.getBoolean("low_latency"), rate)
        }
    }
}
