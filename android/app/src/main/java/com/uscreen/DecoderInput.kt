package com.uscreen

import android.media.MediaCodec

/** One complete access unit. Borrowed on the synchronous path; queued consumers
 * must first detach storage because VideoPacketReader reuses its byte array. */
internal data class DecoderInput(
    val data: ByteArray, val offset: Int, val size: Int,
    val configuration: Boolean, val sequence: Long,
) {
    fun detached() = copy(data = data.copyOfRange(offset, offset + size), offset = 0)

    fun write(codec: MediaCodec, index: Int) {
        val buffer = checkNotNull(codec.getInputBuffer(index)) { "Decoder returned no input buffer" }
        buffer.clear()
        buffer.put(data, offset, size)
        val flags = if (configuration) MediaCodec.BUFFER_FLAG_CODEC_CONFIG else 0
        codec.queueInputBuffer(index, 0, size, sequence, flags)
    }
}
