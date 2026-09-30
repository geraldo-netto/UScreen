package com.blent

import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.concurrent.TimeUnit
import okio.BufferedSource

internal data class AudioBlock(val samples: ShortArray, val discontinuity: Boolean = false, val clock: AudioClockSample? = null)

/** Atomic, bounded receive state for one authenticated connection. */
internal class AudioPackets(private val generation: Long, private val direction: Int, private val clocked: Boolean = false) {
    private var previous: Pair<Long, Long>? = null
    init { require(generation != 0L && direction in 1..2) }
    fun read(source: BufferedSource): AudioBlock {
        source.timeout().timeout(250, TimeUnit.MILLISECONDS).deadline(250, TimeUnit.MILLISECONDS)
        try {
            val fields = header(source.readByteArray(28))
            val clock = if (clocked) AudioClockSample.read(source.readByteArray(24)) else null
            val bytes = source.readByteArray(960L * direction)
            val samples = ShortArray(480 * direction)
            ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN).asShortBuffer().get(samples)
            previous = fields.first to fields.second
            return AudioBlock(samples, fields.third, clock)
        } finally { source.timeout().clearDeadline() }
    }
    private fun header(bytes: ByteArray): Triple<Long, Long, Boolean> {
        val header = ByteBuffer.wrap(bytes).order(ByteOrder.BIG_ENDIAN)
        require(header.long == generation) { "Stale audio session." }
        val sequence = header.long; val timestamp = header.long
        require(sequence != -1L) { "Audio sequence exhausted." }
        require(header.short.toInt() == 960 * direction + (if (clocked) 24 else 0) && header.get().toInt() == direction && header.get().toInt() == 0) { "Invalid audio frame format." }
        val last = previous
        if (last == null) { require(sequence == 0L); return Triple(sequence, timestamp, false) }
        require(java.lang.Long.compareUnsigned(sequence, last.first) > 0 &&
            java.lang.Long.compareUnsigned(timestamp, last.second) > 0) { "Replayed or reordered audio." }
        return Triple(sequence, timestamp, sequence != last.first + 1)
    }
}

internal data class PlaybackChunk(val samples: ShortArray, val at: Long, val discontinuity: Boolean)
