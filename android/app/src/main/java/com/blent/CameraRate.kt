package com.blent

/** Feedback-driven target, not a claim about achieved encoder/network bitrate. */
internal class CameraRate(private val ceiling: Int, minimum: Int,
    private val enabled: Boolean, private val budgetUs: Long) {
    private val floor = minOf(minimum, ceiling)
    var current = ceiling; private set
    private var badSamples = 0
    private var goodSince = -1L
    private var changedAt = 0L

    init {
        require(ceiling in 256..20000 && minimum in 256..20000)
        require(budgetUs in 50_000..2_000_000)
    }

    fun observe(delayUs: Long, nowUs: Long): Int? {
        require(delayUs >= 0 && nowUs >= 0)
        if (!enabled) return null
        return when {
            delayUs >= budgetUs * 3 / 4 -> pressure(nowUs)
            delayUs <= budgetUs / 3 -> healthy(nowUs)
            else -> { badSamples = 0; goodSince = -1; null }
        }
    }

    fun congested(nowUs: Long): Int? {
        require(nowUs >= 0)
        if (!enabled) return null
        return change(maxOf(floor, current * 3 / 4), nowUs)
    }

    private fun pressure(nowUs: Long): Int? {
        goodSince = -1
        badSamples = minOf(3, badSamples + 1)
        if (badSamples < 3 || nowUs - changedAt < 1_000_000) return null
        return congested(nowUs)
    }

    private fun healthy(nowUs: Long): Int? {
        badSamples = 0
        if (goodSince < 0) goodSince = nowUs
        if (nowUs - goodSince < 5_000_000) return null
        return change(minOf(ceiling, current + maxOf(64, current / 10)), nowUs)
    }

    private fun change(target: Int, nowUs: Long): Int? {
        badSamples = 0; goodSince = -1; changedAt = nowUs
        if (target == current) return null
        current = target
        return target
    }
}
