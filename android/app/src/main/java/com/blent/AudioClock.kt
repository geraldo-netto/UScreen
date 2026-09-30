package com.blent

import java.math.BigInteger
import java.nio.ByteBuffer

/** Native frame position and native monotonic time; zero bytes mean unavailable. */
internal data class AudioClockSample(val epoch: Long, val frames: Long, val nanos: Long) {
    fun valid() = epoch > 0 && frames >= 0 && nanos > 0
    fun bytes(): ByteArray {
        require(valid())
        return ByteBuffer.allocate(24).putLong(epoch).putLong(frames).putLong(nanos).array()
    }
    companion object {
        fun read(bytes: ByteArray): AudioClockSample? {
            require(bytes.size == 24)
            if (bytes.all { it == 0.toByte() }) return null
            val input = ByteBuffer.wrap(bytes)
            return AudioClockSample(input.long, input.long, input.long).also { require(it.valid()) }
        }
    }
}

/** Two-to-five-second counter windows. Repeated snapshots do not refresh a rate. */
internal class AudioClockWindow {
    private var base: AudioClockSample? = null
    private var last: AudioClockSample? = null
    private var completedAt = 0L
    private var rate: Pair<Long, Long>? = null
    fun clear() { base = null; last = null; rate = null; completedAt = 0 }
    fun observe(sample: AudioClockSample?, now: Long): Boolean {
        if (sample?.valid() != true) { clear(); return false }
        if (last == sample) return false
        val previous = last
        val reset = previous != null && (sample.epoch != previous.epoch || sample.frames < previous.frames || sample.nanos <= previous.nanos)
        if (reset) clear()
        last = sample
        val first = base ?: sample.also { base = it }
        return reset || complete(first, sample, now)
    }
    private fun complete(first: AudioClockSample, sample: AudioClockSample, now: Long): Boolean {
        val nanos = sample.nanos - first.nanos
        if (nanos < 2_000_000_000L) return false
        base = sample
        val frames = sample.frames - first.frames
        if (nanos > 5_000_000_000L || frames !in 1..480_000L) { rate = null; return true }
        rate = frames to nanos; completedAt = now
        return false
    }
    fun rate(now: Long): Pair<Long, Long>? = if (now >= completedAt && now - completedAt <= 3000) rate else null
}

/** Compare rates through normalized durations, never subtract endpoint clocks. */
internal class AudioClockDrift {
    val source = AudioClockWindow()
    val destination = AudioClockWindow()
    fun clear() { source.clear(); destination.clear() }
    fun correction(now: Long): Int? {
        val sourceRate = source.rate(now) ?: return null
        val destinationRate = destination.rate(now) ?: return null
        val numerator = BigInteger.valueOf(sourceRate.first * destinationRate.second)
        val denominator = BigInteger.valueOf(destinationRate.first * sourceRate.second)
        val difference = (numerator - denominator) * BigInteger.valueOf(1_000_000)
        require(difference.abs() <= denominator * BigInteger.valueOf(1000)) { "Native audio clock drift exceeds 1000 ppm." }
        return (difference / denominator).toInt()
    }
}
