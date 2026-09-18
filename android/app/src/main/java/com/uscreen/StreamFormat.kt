package com.uscreen

import org.json.JSONObject

/** One validated control format for decoder setup and capability queries. */
internal object StreamFormat {
    fun read(message: JSONObject, previous: DecoderFormat?, nativeWidth: Int, nativeHeight: Int, defaultFps: Int): DecoderFormat? = try {
        val mime = if (message.has("codec")) VideoCodec.types[message.getString("codec")]
            else previous?.mimeType ?: VideoReceiver.MIME_TYPE
        requireNotNull(mime) { "Unsupported host codec" }
        val (width, height) = dimensions(message, previous, nativeWidth, nativeHeight)
        val fps = message.optInt("fps", previous?.fps ?: defaultFps)
        require(fps in 10..90) { "Invalid stream FPS" }
        DecoderFormat(mime, width, height, fps)
    } catch (_: Exception) { null }

    private fun dimensions(message: JSONObject, previous: DecoderFormat?, nativeWidth: Int, nativeHeight: Int): Pair<Int, Int> {
        // Old hosts omit both encoded dimensions. Retain the current format on
        // partial updates, or use the panel hint for the first legacy greeting.
        val pixels = if (message.has("video_width") || message.has("video_height")) {
            message.optInt("video_width") to message.optInt("video_height")
        } else if (previous != null) {
            previous.width to previous.height
        } else {
            nativeWidth.takeIf { it in 2..4096 }?.let { it to nativeHeight } ?: (1920 to 1080)
        }
        require(pixels.first in 2..4096 && pixels.second in 2..4096) { "Invalid stream dimensions" }
        return pixels
    }
}
