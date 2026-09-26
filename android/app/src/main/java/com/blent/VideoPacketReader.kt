package com.blent

import android.util.Log
import java.io.EOFException
import java.io.InputStream
import com.blent.VideoReceiver.Companion.FRAME_HEADER_SIZE
import com.blent.VideoReceiver.Companion.MAX_FRAME_SIZE
import com.blent.VideoReceiver.Companion.PACKET_TYPE_CONFIG
import com.blent.VideoReceiver.Companion.PACKET_TYPE_FRAME
import com.blent.VideoReceiver.Companion.TAG

/** Synchronous consumers must finish using the borrowed bytes before read(). */
internal interface VideoPacketSink {
    fun configuration(data: ByteArray, offset: Int, size: Int)
    fun frame(sequence: Int, data: ByteArray, offset: Int, size: Int)
}

/** Length/type/sequence framing, independent of codec, Surface and session state.
 * Storage belongs to this connection and is reused without per-frame allocation. */
internal class VideoPacketReader(private val input: InputStream) {
    private val header = ByteArray(4)
    private var data = ByteArray(512 * 1024)
    var size = 0; private set

    fun read(): Boolean {
        readExact(header, 4)
        size = bigEndianInt(header, 0)
        if (size <= 1 || size > MAX_FRAME_SIZE + 1) {
            Log.w(TAG, "Invalid packet size: $size, reconnecting")
            return false
        }
        if (data.size < size) data = ByteArray(size + size / 2)
        readExact(data, size)
        return true
    }

    fun dispatch(sink: VideoPacketSink): Boolean {
        when (val type = data[0].toInt() and 0xff) {
            PACKET_TYPE_CONFIG -> sink.configuration(data, 1, size - 1)
            PACKET_TYPE_FRAME -> {
                if (size <= FRAME_HEADER_SIZE) {
                    Log.w(TAG, "Truncated frame packet: $size, reconnecting")
                    return false
                }
                sink.frame(bigEndianInt(data, 1), data, FRAME_HEADER_SIZE, size - FRAME_HEADER_SIZE)
            }
            else -> {
                Log.w(TAG, "Unknown packet type: $type, reconnecting")
                return false
            }
        }
        return true
    }

    private fun readExact(buffer: ByteArray, length: Int) {
        var offset = 0
        while (offset < length) {
            val read = input.read(buffer, offset, length - offset)
            if (read < 0) throw EOFException("Stream closed")
            offset += read
        }
    }

    private fun bigEndianInt(bytes: ByteArray, offset: Int): Int =
        ((bytes[offset].toInt() and 0xff) shl 24) or
            ((bytes[offset + 1].toInt() and 0xff) shl 16) or
            ((bytes[offset + 2].toInt() and 0xff) shl 8) or
            (bytes[offset + 3].toInt() and 0xff)
}
