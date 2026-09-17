package com.uscreen

/** Scalar counters only: no retained messages, samples, coordinates or tokens. */
internal class ControlStatistics {
    private var accepted = 0L
    private var rejected = 0L
    private var peakQueueBytes = 0L
    private var oldestSampleMs = 0L
    private var sampledEvents = 0L
    private var totalSampleAgeMs = 0L

    // Access is serialized by ControlSession's shared input lock.
    fun record(ok: Boolean, beforeBytes: Long, afterBytes: Long, sampleAgeMs: Long?) {
        if (ok) accepted++ else rejected++
        peakQueueBytes = maxOf(peakQueueBytes, beforeBytes, afterBytes)
        if (sampleAgeMs != null) {
            val age = sampleAgeMs.coerceAtLeast(0)
            sampledEvents++
            totalSampleAgeMs += age
            oldestSampleMs = maxOf(oldestSampleMs, age)
        }
    }

    fun snapshot(queueBytes: Long) = ControlStatisticsSnapshot(
        accepted, rejected, queueBytes, peakQueueBytes,
        sampledEvents, totalSampleAgeMs, oldestSampleMs
    )
}

internal data class ControlStatisticsSnapshot(
    val accepted: Long,
    val rejected: Long,
    val queueBytes: Long,
    val peakQueueBytes: Long,
    val sampledEvents: Long,
    val totalSampleAgeMs: Long,
    val oldestSampleMs: Long,
)
