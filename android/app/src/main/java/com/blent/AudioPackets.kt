package com.blent

import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.concurrent.TimeUnit
import okio.BufferedSource

internal data class AudioBlock(val samples: ShortArray, val discontinuity: Boolean = false)

/** Atomic, bounded receive state for one authenticated connection. */
internal class AudioPackets(private val generation: Long, private val direction: Int) {
    private var previous: Pair<Long, Long>? = null
    init { require(generation != 0L && direction in 1..2) }
    fun read(source: BufferedSource): AudioBlock {
        source.timeout().timeout(250, TimeUnit.MILLISECONDS).deadline(250, TimeUnit.MILLISECONDS)
        try {
            val fields = header(source.readByteArray(28))
            val bytes = source.readByteArray(960L * direction)
            val samples = ShortArray(480 * direction)
            ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN).asShortBuffer().get(samples)
            previous = fields.first to fields.second
            return AudioBlock(samples, fields.third)
        } finally { source.timeout().clearDeadline() }
    }
    private fun header(bytes: ByteArray): Triple<Long, Long, Boolean> {
        val header = ByteBuffer.wrap(bytes).order(ByteOrder.BIG_ENDIAN)
        require(header.long == generation) { "Stale audio session." }
        val sequence = header.long; val timestamp = header.long
        require(sequence != -1L) { "Audio sequence exhausted." }
        require(header.short.toInt() == 960 * direction && header.get().toInt() == direction && header.get().toInt() == 0) { "Invalid audio frame format." }
        val last = previous
        if (last == null) { require(sequence == 0L); return Triple(sequence, timestamp, false) }
        require(java.lang.Long.compareUnsigned(sequence, last.first) > 0 &&
            java.lang.Long.compareUnsigned(timestamp, last.second) > 0) { "Replayed or reordered audio." }
        return Triple(sequence, timestamp, sequence != last.first + 1)
    }
}

internal data class PlaybackChunk(val samples: ShortArray, val at: Long, val discontinuity: Boolean)

/** Platform-independent queue; sample the adapter clock while holding ownership. */
internal class AudioPlaybackQueue(targetMs: Int, private val clock: () -> Long) {
    private val target = targetMs / 10
    private val blocks = java.util.ArrayDeque<PlaybackChunk>(20)
    private var primed = false
    private var discontinuity = false
    private var lastTime = 0L
    init { require(targetMs in 20..200 && targetMs % 10 == 0) }
    @Synchronized fun offer(block: AudioBlock) {
        require(block.samples.size == 960)
        val now = advance()
        if (block.discontinuity) clear()
        if (blocks.size == 20) { blocks.removeFirst(); discontinuity = true }
        blocks.addLast(PlaybackChunk(block.samples, now, block.discontinuity))
    }
    @Synchronized fun poll(): PlaybackChunk? {
        advance()
        if (!primed && blocks.size < target) return null
        val block = blocks.pollFirst()
        primed = block != null
        if (block == null) return null
        val result = block.copy(discontinuity = block.discontinuity || discontinuity)
        discontinuity = false
        return result
    }
    @Synchronized fun clear() { blocks.clear(); primed = false; discontinuity = true }
    private fun advance(): Long {
        val now = clock()
        require(now >= lastTime) { "Audio clock moved backwards." }
        lastTime = now
        while (blocks.peekFirst()?.let { now - it.at > 200 } == true) {
            blocks.removeFirst(); primed = false; discontinuity = true
        }
        return now
    }
}
