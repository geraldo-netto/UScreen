package com.uscreen

import android.media.MediaCodecList
import android.media.MediaCodecInfo
import android.media.MediaFormat
import android.os.Build
import java.nio.ByteBuffer
import java.nio.ByteOrder
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import org.json.JSONArray
import org.json.JSONObject

/** Wire identity and framed codec configuration; unrelated to the host muxer. */
internal object VideoCodec {
    val types = mapOf("h264" to "video/avc", "hevc" to "video/hevc", "vp9" to "video/x-vnd.on2.vp9", "av1" to "video/av01")
    fun hasConfigurationHeader(data: ByteArray, offset: Int, size: Int): Boolean =
        size >= 4 && ByteBuffer.wrap(data, offset, 4).int == 0x55534331
    private val framedTypes = mapOf(3 to types.getValue("vp9"), 4 to types.getValue("av1"))
    fun framed(mime: String) = mime in framedTypes.values

    fun configuration(data: ByteArray, offset: Int, size: Int, mime: String, fps: Int): DecoderFormat {
        require(size in 13..65549) { "Invalid framed codec configuration length" }
        val bytes = ByteBuffer.wrap(data, offset, size).order(ByteOrder.BIG_ENDIAN)
        require(bytes.int == 0x55534331) { "Unsupported video configuration version" }
        require(framedTypes[bytes.get().toInt()] == mime) { "Video/control codec mismatch" }
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
        val decoders = VideoCodec.types.values.associateWith { mime -> compatible(mime, width, height, fps) }
        describe(width, height, fps) { mime, _, _, _ ->
            decoders[mime]?.isNotEmpty() == true
        }.apply {
            put("hardware", JSONArray(VideoCodec.types.filterValues { mime -> decoders[mime]?.any(::hardware) == true }.keys.toList()))
            val entries = VideoCodec.types.flatMap { (codec, mime) ->
                decoders[mime].orEmpty().mapNotNull { MediaInventory.describe(it, codec, width, height, fps) }
            }.distinctBy { it.getString("codec") to it.getString("name") }.take(16)
            put("details", JSONArray(entries))
        }
    }
    internal fun describe(width: Int, height: Int, fps: Int, supports: (String, Int, Int, Int) -> Boolean): JSONObject {
        val names = VideoCodec.types.filter { (_, mime) -> supports(mime, width, height, fps) }.keys
        return JSONObject().apply {
            put("protocol", 1); put("width", width); put("height", height); put("fps", fps)
            put("codecs", JSONArray(names.toList()))
        }
    }
    internal fun hardware(info: MediaCodecInfo): Boolean =
        Build.VERSION.SDK_INT >= 29 && info.isHardwareAccelerated

    private fun compatible(mime: String, width: Int, height: Int, fps: Int): List<MediaCodecInfo> = try {
        val format = MediaFormat.createVideoFormat(mime, width, height)
        format.setInteger(MediaFormat.KEY_FRAME_RATE, fps)
        compatible(format, mime)
    } catch (_: Exception) { emptyList() }

    internal fun decoderName(format: MediaFormat, mime: String): String? {
        val choices = compatible(format, mime)
        return (choices.firstOrNull(::hardware) ?: choices.firstOrNull())?.name
    }
    private fun compatible(format: MediaFormat, mime: String): List<MediaCodecInfo> =
        MediaCodecList(MediaCodecList.REGULAR_CODECS).codecInfos.filter { info ->
            try { !info.isEncoder && info.getCapabilitiesForType(mime).isFormatSupported(format) }
            catch (_: Exception) { false }
        }
}
