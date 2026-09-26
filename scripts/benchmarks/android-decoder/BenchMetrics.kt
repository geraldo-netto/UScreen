package com.blent

import android.media.MediaCodec
import java.util.concurrent.atomic.AtomicLong
import java.util.concurrent.atomic.AtomicLongArray
import org.json.JSONArray
import org.json.JSONObject

/** Identical instrumentation replaces the native dequeue call sites in both
 * source snapshots. Counts are calls, not inferred scheduler wakeups. */
internal object BenchMetrics {
    private val timingHits = AtomicLong()
    private val timingMisses = AtomicLong()
    fun timingLookup(hit: Boolean) { if (hit) timingHits.incrementAndGet() else timingMisses.incrementAndGet() }
    private val inputs = AtomicLong()
    private val outputs = AtomicLong()
    private val inputNanos = AtomicLong()
    private val outputNanos = AtomicLong()
    private val arrivals = AtomicLongArray(65536)
    private val syntheticSources = AtomicLongArray(65536)
    private val discards = AtomicLongArray(65536)
    private val releases = AtomicLongArray(65536)
    private val notifications = AtomicLongArray(65536)
    private val renderedTimes = AtomicLongArray(65536)
    private val acknowledgements = AtomicLongArray(65536)
    fun arrived(sequence: Int) { arrivals.set(sequence, System.nanoTime()) }
    fun sourceReady(sequence: Int, nanos: Long) { syntheticSources.set(sequence, nanos) }
    fun discarded(sequence: Int) { discards.set(sequence, System.nanoTime()) }
    fun released(sequence: Int) { releases.set(sequence, System.nanoTime()) }
    fun notified(sequence: Int, renderedNanos: Long) {
        notifications.set(sequence, System.nanoTime())
        renderedTimes.set(sequence, renderedNanos)
    }
    fun acknowledged(sequence: Int) { acknowledgements.set(sequence, System.nanoTime()) }
    fun trace(first: Int, end: Int): JSONArray = JSONArray().apply {
        for (sequence in first until end) {
            put(JSONArray().put(sequence).put(arrivals.get(sequence)).put(releases.get(sequence))
                .put(notifications.get(sequence)).put(renderedTimes.get(sequence)).put(acknowledgements.get(sequence))
                .put(syntheticSources.get(sequence)).put(discards.get(sequence)))
        }
    }
    fun input(codec: MediaCodec, timeout: Long): Int {
        val start = System.nanoTime()
        try { return codec.dequeueInputBuffer(timeout) }
        finally { inputNanos.addAndGet(System.nanoTime() - start); inputs.incrementAndGet() }
    }
    fun output(codec: MediaCodec, info: MediaCodec.BufferInfo, timeout: Long): Int {
        val start = System.nanoTime()
        try { return codec.dequeueOutputBuffer(info, timeout) }
        finally { outputNanos.addAndGet(System.nanoTime() - start); outputs.incrementAndGet() }
    }
    fun snapshot() = JSONObject().put("input_calls", inputs.get()).put("output_calls", outputs.get())
        .put("input_ns", inputNanos.get()).put("output_ns", outputNanos.get())
        .put("timing_cache_hits", timingHits.get()).put("timing_cache_misses", timingMisses.get())
}
