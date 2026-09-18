package com.uscreen

import android.media.MediaCodecList
import android.media.MediaFormat
import java.nio.ByteBuffer
import java.nio.ByteOrder
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import org.json.JSONArray
import org.json.JSONObject

/** Wire identity and framed codec configuration; unrelated to the host muxer. */
internal object VideoCodec {
    val types = mapOf("h264" to "video/avc", "hevc" to "video/hevc", "vp9" to "video/x-vnd.on2.vp9")
    fun hasConfigurationHeader(data: ByteArray, offset: Int, size: Int): Boolean =
        size >= 4 && ByteBuffer.wrap(data, offset, 4).int == 0x55534331
    fun framed(mime: String) = mime == types["vp9"]

    fun configuration(data: ByteArray, offset: Int, size: Int, mime: String, fps: Int): DecoderFormat {
        require(size in 13..65549) { "Invalid framed codec configuration length" }
        val bytes = ByteBuffer.wrap(data, offset, size).order(ByteOrder.BIG_ENDIAN)
        require(bytes.int == 0x55534331) { "Unsupported video configuration version" }
        require(bytes.get().toInt() == 3 && mime == types["vp9"]) { "Video/control codec mismatch" }
        val width = bytes.int
        val height = bytes.int
        require(width in 2..4096 && height in 2..4096) { "Invalid stream dimensions" }
        val private = ByteArray(bytes.remaining()).also { bytes.get(it) }
        return DecoderFormat(mime, width, height, fps, private)
    }
}

/** Serialized off the Activity/control locks. Advertised support is not a speed measurement. */
internal object DecoderCapabilities {
    private val admission = Mutex()
    suspend fun report(width: Int, height: Int, fps: Int): JSONObject = admission.withLock {
        describe(width, height, fps, ::supported)
    }
    internal fun describe(width: Int, height: Int, fps: Int, supports: (String, Int, Int, Int) -> Boolean): JSONObject {
        val names = VideoCodec.types.filter { (_, mime) -> supports(mime, width, height, fps) }.keys
        return JSONObject().apply {
            put("protocol", 1); put("width", width); put("height", height); put("fps", fps)
            put("codecs", JSONArray(names.toList()))
        }
    }
    private fun supported(mime: String, width: Int, height: Int, fps: Int): Boolean = try {
        val format = MediaFormat.createVideoFormat(mime, width, height)
        format.setInteger(MediaFormat.KEY_FRAME_RATE, fps)
        MediaCodecList(MediaCodecList.REGULAR_CODECS).findDecoderForFormat(format) != null
    } catch (_: Exception) { false }
}
