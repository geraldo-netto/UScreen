package com.uscreen

import android.media.MediaCodec
import java.nio.ByteBuffer

internal interface DecoderInputInfo {
    val size: Int
    val configuration: Boolean
    val sequence: Long
}

internal fun queueCodecInput(codec: MediaCodec, index: Int, info: DecoderInputInfo, fill: (ByteBuffer) -> Unit) {
    val buffer = checkNotNull(codec.getInputBuffer(index)) { "Decoder returned no input buffer" }
    require(info.size in 1..buffer.capacity()) { "Access unit exceeds codec input capacity" }
    buffer.clear().limit(info.size)
    fill(buffer)
    check(buffer.position() == info.size) { "Incomplete codec input payload" }
    val flags = if (info.configuration) MediaCodec.BUFFER_FLAG_CODEC_CONFIG else 0
    codec.queueInputBuffer(index, 0, info.size, info.sequence, flags)
}

/** One complete access unit. Borrowed on the synchronous path; queued consumers
 * must first detach storage because VideoPacketReader reuses its byte array. */
internal data class DecoderInput(
    val data: ByteArray, val offset: Int, override val size: Int,
    override val configuration: Boolean, override val sequence: Long,
) : DecoderInputInfo {
    fun detached() = copy(data = data.copyOfRange(offset, offset + size), offset = 0)

    fun write(codec: MediaCodec, index: Int) {
        queueCodecInput(codec, index, this) { it.put(data, offset, size) }
    }
}
