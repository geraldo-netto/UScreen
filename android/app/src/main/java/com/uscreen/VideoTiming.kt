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
    /**
     * seq → nanoTime the frame finished arriving, so the render callback can
     * report the arrival-to-callback portion of the host’s send-to-ack interval
     * rather than on the wire. Bounded and cheap: a plain ring, since frames
     * are rendered in the order they arrive.
     */
    private val arrivalSeq = IntArray(ARRIVAL_RING)
    private val arrivalNanos = LongArray(ARRIVAL_RING)
    @Volatile private var arrivalWrite = 0

    /**
     * Splits arrival-to-output-release from output-release-to-render-callback.
     * The latter includes callback scheduling; it does not isolate composition
     * or measure when pixels became visible. Both boundaries use this process's
     * nanoTime samples, not the render timestamp supplied by MediaCodec.
     */
    private val releaseNanos = LongArray(ARRIVAL_RING)
    private var decodeSumUs = 0L
    private var presentSumUs = 0L
    private var splitCount = 0
    private var lastSplitLogNanos = 0L

    fun noteReleased(seq: Int) {
        for (n in 0 until ARRIVAL_RING) {
            val i = (arrivalWrite - 1 - n + ARRIVAL_RING * 2) % ARRIVAL_RING
            if (arrivalSeq[i] == seq) {
                releaseNanos[i] = clock()
                return
            }
        }
    }

    fun noteArrival(seq: Int) {
        val i = arrivalWrite % ARRIVAL_RING
        arrivalSeq[i] = seq
        arrivalNanos[i] = clock()
        // Keep a ring index: an unbounded Int eventually overflows in lookups.
        arrivalWrite = (i + 1) % ARRIVAL_RING
    }

    /** Microseconds from frame arrival to render-callback execution, or -1. */
    fun decodeMicrosFor(seq: Int): Int {
        for (n in 0 until ARRIVAL_RING) {
            val i = (arrivalWrite - 1 - n + ARRIVAL_RING * 2) % ARRIVAL_RING
            if (arrivalSeq[i] == seq && arrivalNanos[i] != 0L) {
                val now = clock()
                val total = ((now - arrivalNanos[i]) / 1000L)
                    .coerceIn(0L, Int.MAX_VALUE.toLong()).toInt()

                // Attribute the time: decode = arrival → buffer released,
                // present = released → callback execution, including dispatch delay.
                val rel = releaseNanos[i]
                if (rel > arrivalNanos[i]) {
                    decodeSumUs += (rel - arrivalNanos[i]) / 1000L
                    presentSumUs += (now - rel) / 1000L
                    splitCount++
                    if (now - lastSplitLogNanos > 5_000_000_000L && splitCount > 0) {
                        Log.i(
                            TAG,
                            "on-device split: decode ${decodeSumUs / splitCount / 1000.0}ms " +
                                "present ${presentSumUs / splitCount / 1000.0}ms " +
                                "($splitCount frames)"
                        )
                        lastSplitLogNanos = now
                        decodeSumUs = 0; presentSumUs = 0; splitCount = 0
                    }
                }
                return total
            }
        }
        return -1
    }

}
