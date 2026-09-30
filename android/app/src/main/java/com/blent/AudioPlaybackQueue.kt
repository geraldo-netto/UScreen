package com.blent

/** Bounded stereo interpolation; native counters and queue age use separate clocks. */
internal class AudioPlaybackQueue(targetMs: Int, private val clock: () -> Long) {
    private val target = targetMs / 10
    private val blocks = java.util.ArrayDeque<PlaybackChunk>(20)
    private val drift = AudioClockDrift()
    private var primed = false
    private var discontinuity = false
    private var lastTime = 0L
    private var offset = 0
    private var phase = 0L
    private var ppm = 0
    private var measured: Int? = null
    init { require(targetMs in 20..200 && targetMs % 10 == 0) }
    @Synchronized fun offer(block: AudioBlock) {
        require(block.samples.size == 960)
        val now = advance()
        if (block.discontinuity) clear()
        if (drift.source.observe(block.clock, now)) { clear(); drift.source.observe(block.clock, now) }
        if (blocks.size == 20) dropOldest()
        blocks.addLast(PlaybackChunk(block.samples, now, block.discontinuity))
    }
    @Synchronized fun nativeClock(sample: AudioClockSample?) {
        val now = advance()
        if (drift.destination.observe(sample, now)) clear()
        try { measured = drift.correction(now); ppm = measured ?: 0 }
        catch (_: IllegalArgumentException) { clear() }
    }
    @Synchronized fun driftDescription(): String = measured?.let { "Native drift correction: $it ppm" }
        ?: "Native clock drift unmeasured (warming, reset or unavailable)"
    @Synchronized fun poll(): PlaybackChunk? {
        advance()
        if (!primed && blocks.size < target) return null
        primed = true
        val step = 1_000_000L + ppm
        val end = phase + 480 * step
        val needed = (phase + 479 * step + 999_999) / 1_000_000 + 1
        if (blocks.size * 480 - offset < maxOf(needed, end / 1_000_000)) { clear(); return null }
        val at = blocks.first.at
        val result = PlaybackChunk(interpolate(step), at, discontinuity)
        consume((end / 1_000_000).toInt())
        phase = end % 1_000_000
        discontinuity = false
        return result
    }
    private fun interpolate(step: Long): ShortArray {
        val samples = ShortArray(960)
        // Snapshot at most 20 references; the arrays remain queue-owned during this call.
        val source = blocks.toTypedArray()
        for (frame in 0 until 480) {
            val position = phase + frame * step
            val index = position / 1_000_000
            val fraction = position % 1_000_000
            for (channel in 0..1) {
                val first = sample(source, index.toInt(), channel)
                val next = sample(source, index.toInt() + if (fraction != 0L) 1 else 0, channel)
                samples[frame * 2 + channel] = (first + (next - first) * fraction / 1_000_000).toShort()
            }
        }
        return samples
    }
    private fun sample(source: Array<PlaybackChunk>, frame: Int, channel: Int): Long {
        val at = offset + frame
        return source[at / 480].samples[(at % 480) * 2 + channel].toLong()
    }
    private fun consume(frames: Int) {
        offset += frames
        while (offset >= 480) { blocks.removeFirst(); offset -= 480 }
    }
    private fun dropOldest() {
        blocks.removeFirst(); offset = 0; phase = 0; discontinuity = true
    }
    @Synchronized fun clear() {
        blocks.clear(); primed = false; discontinuity = true
        offset = 0; phase = 0; ppm = 0; measured = null; drift.clear()
    }
    private fun advance(): Long {
        val now = clock()
        require(now >= lastTime) { "Audio clock moved backwards." }
        lastTime = now
        while (blocks.peekFirst()?.let { now - it.at > 200 } == true) {
            dropOldest(); primed = false
        }
        return now
    }
}
