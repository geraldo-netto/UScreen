package com.blent

import org.junit.Assert.*
import org.junit.Test

class CameraFreshnessTest {
    @Test fun t616_dropChainRequiresFreshKeyframeAndBoundsSyncWait() {
        val policy = CameraFreshness(150_000)
        assertEquals(CameraFreshness.Decision.REQUEST_SYNC, policy.choose(0, false, 0))
        assertEquals(CameraFreshness.Decision.DROP, policy.choose(0, false, 1))
        assertEquals(CameraFreshness.Decision.DROP, policy.choose(150_000, true, 2))
        assertEquals(CameraFreshness.Decision.SEND, policy.choose(149_999, true, 3))
        assertEquals(CameraFreshness.Decision.SEND, policy.choose(0, false, 4))
        assertEquals(CameraFreshness.Decision.REQUEST_SYNC, policy.choose(150_000, false, 5))
        assertTrue(runCatching { policy.choose(0, false, 2_000_005) }.isFailure)
        assertEquals(5L, policy.dropped)
        assertTrue(runCatching { policy.choose(-1, true, 0) }.isFailure)
        assertTrue(runCatching { policy.choose(0, true, -1) }.isFailure)
        for (budget in listOf(-1L, 0, 49_999, 50_000, 150_000, 2_000_000, 2_000_001, Long.MAX_VALUE))
            assertEquals(budget in 50_000..2_000_000, runCatching { CameraFreshness(budget) }.isSuccess)
    }
}
