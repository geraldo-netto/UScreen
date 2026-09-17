package com.uscreen

/** Output progress shared by the codec input and output workers. */
internal class DecoderOutputWatchdog {
    companion object {
        private const val WINDOW_NANOS = 1_500_000_000L
        private const val MIN_FRAMES = 4
    }
    data class Stall(val queued: Int, val silentNanos: Long, val droppedHints: Boolean)

    private var queued = 0
    private var lastOutput = 0L
    private var recoveryStart = 0L
    private var recoveryFrames = 0
    private var stalls = 0
    @Volatile var lowLatencyHints = true; private set

    // Reconnection needs a fresh observation window, but must retain failures.
    @Synchronized fun restarted(now: Long) {
        queued = 0
        lastOutput = now
        recoveryFrames = 0
    }

    @Synchronized fun output(now: Long) {
        if (recoveryFrames == 0 || now - lastOutput > WINDOW_NANOS) {
            recoveryStart = now
            recoveryFrames = 0
        }
        lastOutput = now
        queued = 0
        recoveryFrames = (recoveryFrames + 1).coerceAtMost(MIN_FRAMES)
        // T339: one output frame (or a brief burst) cannot clear a stall streak.
        // Require progress spanning a watchdog window without a silent gap.
        if (recoveryFrames >= MIN_FRAMES && now - recoveryStart >= WINDOW_NANOS) {
            stalls = 0
        }
    }

    @Synchronized fun queued(now: Long): Stall? {
        queued++
        val silentNanos = now - lastOutput
        if (queued < MIN_FRAMES || silentNanos <= WINDOW_NANOS) return null
        stalls = (stalls + 1).coerceAtMost(2)
        val dropHints = stalls >= 2 && lowLatencyHints
        if (dropHints) lowLatencyHints = false
        return Stall(queued, silentNanos, dropHints)
    }
}
