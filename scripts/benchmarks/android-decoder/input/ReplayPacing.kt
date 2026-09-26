package com.blent.benchmark

/** Synthetic delivery batching, independent of codec/frame contents. */
internal object ReplayPacing {
    fun deliveryIndex(index: Int, count: Int, burst: Int): Int {
        require(index in 0 until count && burst in 1..32)
        return minOf((index / burst + 1) * burst - 1, count - 1)
    }
}
