package com.blent.benchmark

/** T419: bounded long replay storage; no growth with an untrusted duration. */
internal object RectLimits {
    fun traceElements(rate: Int, seconds: Int): Int {
        require(rate == 5 || rate == 60)
        require(seconds in 1..600)
        return rate * seconds * 8
    }
}
