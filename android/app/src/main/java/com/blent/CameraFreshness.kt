package com.blent

/** Constant-space admission. Dropping any reference invalidates dependent frames. */
internal class CameraFreshness(private val budgetUs: Long) {
    enum class Decision { SEND, DROP, REQUEST_SYNC }
    private var waiting = true
    private var requestedAt = -1L
    var dropped = 0L; private set

    init { require(budgetUs in 50_000..2_000_000) }

    fun choose(ageUs: Long, keyframe: Boolean, nowUs: Long): Decision {
        require(ageUs >= 0 && nowUs >= 0)
        if (ageUs >= budgetUs) waiting = true
        if (waiting && keyframe && ageUs < budgetUs) { waiting = false; requestedAt = -1 }
        if (!waiting) return Decision.SEND
        dropped++
        if (requestedAt < 0) { requestedAt = nowUs; return Decision.REQUEST_SYNC }
        check(nowUs - requestedAt < 2_000_000) { "Camera encoder did not produce a fresh keyframe" }
        return Decision.DROP
    }
}
