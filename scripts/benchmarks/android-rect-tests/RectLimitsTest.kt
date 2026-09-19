package com.uscreen.benchmark

import org.junit.Assert.*
import org.junit.Test

class RectLimitsTest {
    @Test fun t419SustainedReplayRemainsBounded() {
        assertEquals(288_000, RectLimits.traceElements(60, 600))
        assertEquals(19_200, RectLimits.traceElements(5, 480))
    }
    @Test fun t419RejectsOverflowingOrUnsupportedRequests() {
        for (duration in listOf(0, 601, Int.MAX_VALUE)) {
            assertThrows(IllegalArgumentException::class.java) { RectLimits.traceElements(60, duration) }
        }
        assertThrows(IllegalArgumentException::class.java) { RectLimits.traceElements(90, 600) }
    }
}
