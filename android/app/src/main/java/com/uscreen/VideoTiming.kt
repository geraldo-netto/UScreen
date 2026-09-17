package com.uscreen

import android.util.Log
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicLong
import com.uscreen.VideoReceiver.Companion.ARRIVAL_RING
import com.uscreen.VideoReceiver.Companion.TAG

/** Counters and published rates for one receiver run. */
internal class ReceiverStatistics {
    private val frames = AtomicInteger(0)
    private val bytes = AtomicLong(0)
    private var sampledAtNanos = System.nanoTime()
    @Volatile var fps = 0f; private set
    @Volatile var mbps = 0f; private set

    fun frameRendered() { frames.incrementAndGet() }
    fun bytesReceived(count: Int) { bytes.addAndGet(count.toLong()) }
    fun sample(nowNanos: Long = System.nanoTime()) {
        val elapsed = nowNanos - sampledAtNanos
        if (elapsed <= 0) return
        sampledAtNanos = nowNanos
        val seconds = elapsed / 1_000_000_000.0
        fps = (frames.getAndSet(0) / seconds).toFloat()
        mbps = (bytes.getAndSet(0) * 8.0 / seconds / 1_000_000.0).toFloat()
    }
}

/** Timing state; independent of transport and MediaCodec ownership. */
internal class FrameTiming(private val clock: () -> Long = System::nanoTime) {
    internal class Epoch
    @Volatile private var epoch = Epoch()
    private val arrivalSeq = IntArray(ARRIVAL_RING)
    private val arrivalNanos = LongArray(ARRIVAL_RING)
    private val releaseNanos = LongArray(ARRIVAL_RING)
    private val valid = BooleanArray(ARRIVAL_RING)
    private val splitRecorded = BooleanArray(ARRIVAL_RING)
    // Fast tag cache; collisions fall back to the original newest-first history.
    // Keep history by arrival count, including sparse and duplicate sequences.
    private val lookup = IntArray(ARRIVAL_RING) { -1 }
    private var arrivalWrite = 0
    private var decodeSumUs = 0L
    private var presentSumUs = 0L
    private var splitCount = 0
    private var lastSplitLogNanos = 0L

    fun currentEpoch(): Epoch = epoch
    @Synchronized fun beginEpoch(): Epoch {
        valid.fill(false)
        lookup.fill(-1)
        arrivalWrite = 0
        decodeSumUs = 0; presentSumUs = 0; splitCount = 0
        lastSplitLogNanos = 0
        return Epoch().also { epoch = it }
    }
    @Synchronized fun retire(expectedEpoch: Epoch) {
        if (epoch === expectedEpoch) beginEpoch()
    }

    fun noteArrival(seq: Int, expectedEpoch: Epoch = epoch, atNanos: Long = clock()) {
        synchronized(this) {
            if (epoch !== expectedEpoch) return
            val i = arrivalWrite and (ARRIVAL_RING - 1)
            arrivalSeq[i] = seq
            arrivalNanos[i] = atNanos
            releaseNanos[i] = 0
            splitRecorded[i] = false
            valid[i] = true
            lookup[seq and (ARRIVAL_RING - 1)] = i
            arrivalWrite = (i + 1) and (ARRIVAL_RING - 1)
        }
    }

    fun noteReleased(seq: Int, expectedEpoch: Epoch = epoch) {
        // Sample before locking, then validate identity while publishing. A
        // delayed callback must not stamp a slot overwritten while it waited.
        val now = clock()
        synchronized(this) {
            if (epoch !== expectedEpoch) return
            val i = find(seq)
            if (i >= 0) releaseNanos[i] = now
        }
    }

    /** Microseconds from complete arrival to callback execution, or -1. */
    fun decodeMicrosFor(seq: Int, expectedEpoch: Epoch = epoch): Int {
        val now = clock()
        var report: String? = null
        val total = synchronized(this) {
            if (epoch !== expectedEpoch) return -1
            val i = find(seq)
            if (i < 0) return -1
            report = recordSplit(i, now)
            ((now - arrivalNanos[i]) / 1000L).coerceIn(0L, Int.MAX_VALUE.toLong()).toInt()
        }
        // Android logging does not hold the timing-state monitor.
        report?.let { Log.i(TAG, it) }
        return total
    }

    private fun find(seq: Int): Int {
        val cached = lookup[seq and (ARRIVAL_RING - 1)]
        if (cached >= 0 && valid[cached] && arrivalSeq[cached] == seq) return cached
        for (n in 0 until ARRIVAL_RING) {
            val i = (arrivalWrite - 1 - n) and (ARRIVAL_RING - 1)
            if (valid[i] && arrivalSeq[i] == seq) return i
        }
        return -1
    }

    private fun recordSplit(i: Int, now: Long): String? {
        val release = releaseNanos[i]
        if (splitRecorded[i] || release <= arrivalNanos[i] || now < release) return null
        splitRecorded[i] = true
        decodeSumUs += (release - arrivalNanos[i]) / 1000L
        presentSumUs += (now - release) / 1000L
        splitCount++
        if (now - lastSplitLogNanos <= 5_000_000_000L) return null
        val report = "on-device split: decode ${decodeSumUs / splitCount / 1000.0}ms " +
            "present ${presentSumUs / splitCount / 1000.0}ms ($splitCount frames)"
        lastSplitLogNanos = now
        decodeSumUs = 0; presentSumUs = 0; splitCount = 0
        return report
    }
}
