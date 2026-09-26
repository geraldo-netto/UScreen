package com.blent

import android.media.MediaCodecInfo.CodecProfileLevel as Levels
import org.json.JSONObject

/** Standard wire vocabulary, independent of Android's profile/level integer values. */
internal object MediaProfiles {
    data class Profile(val name: String, val depth: Int, val android: Int)
    private val profiles = mapOf(
        "h264" to listOf(Profile("baseline", 8, Levels.AVCProfileBaseline),
            Profile("constrained-baseline", 8, Levels.AVCProfileConstrainedBaseline),
            Profile("main", 8, Levels.AVCProfileMain), Profile("high", 8, Levels.AVCProfileHigh)),
        "hevc" to listOf(Profile("main", 8, Levels.HEVCProfileMain), Profile("main10", 10, Levels.HEVCProfileMain10)),
        "vp9" to listOf(Profile("profile0", 8, Levels.VP9Profile0), Profile("profile2", 10, Levels.VP9Profile2)),
        "av1" to listOf(Profile("main", 8, Levels.AV1ProfileMain8), Profile("main", 10, Levels.AV1ProfileMain10)),
    )
    private val levels = mapOf(
        "h264" to listOf(10, 9, 11, 12, 13, 20, 21, 22, 30, 31, 32, 40, 41, 42, 50, 51, 52, 60, 61, 62),
        "hevc" to listOf(10, 20, 21, 30, 31, 40, 41, 50, 51, 52, 60, 61, 62),
        "vp9" to listOf(10, 11, 20, 21, 30, 31, 40, 41, 50, 51, 52, 60, 61, 62),
        "av1" to (2..7).flatMap { major -> (0..3).map { major * 10 + it } },
    )
    fun profile(codec: String, android: Int): Profile? = profiles[codec]?.firstOrNull { it.android == android }
    fun profile(codec: String, name: String, depth: Int): Profile? = profiles[codec]?.firstOrNull { it.name == name && it.depth == depth }
    fun validLevel(codec: String, level: Int) = levels[codec]?.contains(level) == true

    fun level(codec: String, android: Int): Int? {
        if (android <= 0 || Integer.bitCount(android) != 1) return null
        val bit = Integer.numberOfTrailingZeros(android)
        // High-tier HEVC capability alone does not authorize main-tier output.
        if (codec == "hevc" && bit % 2 != 0) return null
        return levels[codec]?.getOrNull(if (codec == "hevc") bit / 2 else bit)
    }

    fun describe(codec: String, pair: Levels): JSONObject? {
        val profile = profile(codec, pair.profile) ?: return null
        val level = level(codec, pair.level) ?: return null
        return JSONObject().put("profile", profile.name).put("depth", profile.depth).put("level", level)
    }

    fun covers(advertised: JSONObject, required: JSONObject): Boolean {
        val profile = advertised.optString("profile") == required.optString("profile") ||
            (advertised.optString("profile") == "baseline" && required.optString("profile") == "constrained-baseline")
        return profile && advertised.optInt("depth") == required.optInt("depth") &&
            levelOrder(advertised.optInt("level")) >= levelOrder(required.optInt("level"))
    }
    private fun levelOrder(level: Int) = if (level == 9) 105 else level * 10
}
