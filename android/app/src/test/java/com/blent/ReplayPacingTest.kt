package com.blent

import com.blent.benchmark.ReplayPacing
import org.junit.Assert.*
import org.junit.Test

class ReplayPacingTest {
    @Test fun t399_default_cadence_preserves_every_original_deadline() {
        for (index in 0 until 900) assertEquals(index, ReplayPacing.deliveryIndex(index, 900, 1))
    }
    @Test fun t399_burst_waits_for_last_source_frame_without_omitting_or_reordering() {
        val deadlines = (0 until 14).map { ReplayPacing.deliveryIndex(it, 14, 6) }
        assertEquals(List(6) { 5 } + List(6) { 11 } + List(2) { 13 }, deadlines)
        assertTrue(deadlines.withIndex().all { (index, delivered) -> delivered >= index })
    }
    @Test fun t399_invalid_indices_and_bursts_cannot_fabricate_deadlines() {
        for ((index, count, burst) in listOf(Triple(1, 1, 1), Triple(-1, 1, 1), Triple(0, 1, 0), Triple(0, 1, 33))) {
            assertThrows(IllegalArgumentException::class.java) { ReplayPacing.deliveryIndex(index, count, burst) }
        }
    }
}
