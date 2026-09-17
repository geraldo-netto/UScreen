package com.uscreen

import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicLong
import java.util.concurrent.atomic.AtomicReference
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34])
class FrameTimingTest {
    private fun splitCount(timing: FrameTiming): Int = timing.javaClass
        .getDeclaredField("splitCount").apply { isAccessible = true }.getInt(timing)

    @Test fun t404_oldReleaseCannotStampAReusedArrivalSlot() {
        val now = AtomicLong(100_000)
        val entered = CountDownLatch(1)
        val resume = CountDownLatch(1)
        val failure = AtomicReference<Throwable?>()
        val timing = FrameTiming {
            if (Thread.currentThread().name == "t404-old-release") {
                entered.countDown()
                check(resume.await(3, TimeUnit.SECONDS))
            }
            now.get()
        }
        timing.noteArrival(7)
        val worker = Thread({
            try { timing.noteReleased(7) } catch (error: Throwable) { failure.set(error) }
        }, "t404-old-release").apply { start() }
        try {
            assertTrue(entered.await(3, TimeUnit.SECONDS))
            now.set(200_000)
            for (sequence in 8..(7 + VideoReceiver.ARRIVAL_RING)) timing.noteArrival(sequence)
            now.set(300_000)
        } finally {
            resume.countDown()
            worker.join(3_000)
        }
        assertFalse(worker.isAlive)
        failure.get()?.let { throw AssertionError("T404 release worker", it) }
        now.set(400_000)
        assertEquals(200, timing.decodeMicrosFor(7 + VideoReceiver.ARRIVAL_RING))
        assertEquals("T404: stale release contaminated the replacement frame", 0, splitCount(timing))
    }

    @Test fun t404_newEpochRejectsStaleTimingAndReusedSequences() {
        var now = 100_000L
        val timing = FrameTiming { now }
        val old = timing.currentEpoch()
        timing.noteArrival(9, old)
        val current = timing.beginEpoch()
        assertEquals("T404: old arrival survived decoder replacement", -1, timing.decodeMicrosFor(9, current))
        now = 200_000L
        timing.noteArrival(9, current)
        now = 300_000L
        timing.noteReleased(9, old)
        timing.noteArrival(10, old)
        assertEquals(-1, timing.decodeMicrosFor(9, old))
        assertEquals(-1, timing.decodeMicrosFor(10, current))
        now = 400_000L
        assertEquals(200, timing.decodeMicrosFor(9, current))
        assertEquals(0, splitCount(timing))
    }

    @Test fun t404_duplicateCallbackDoesNotDoubleCountDiagnosticSplit() {
        var now = 100_000L
        val timing = FrameTiming { now }
        timing.noteArrival(9)
        now = 200_000L
        timing.noteReleased(9)
        now = 300_000L
        assertEquals(200, timing.decodeMicrosFor(9))
        now = 400_000L
        assertEquals(300, timing.decodeMicrosFor(9))
        assertEquals("T404: duplicated callback biased split averages", 1, splitCount(timing))
    }

    @Test fun t404_zeroMonotonicOriginStillRepresentsAnArrival() {
        var now = 0L
        val timing = FrameTiming { now }
        timing.noteArrival(9)
        now = 100_000L
        assertEquals(100, timing.decodeMicrosFor(9))
    }


    @Test fun t404_lookupCollisionsPreserveSparseAndDuplicateArrivalHistory() {
        var now = 100_000L
        val timing = FrameTiming { now }
        timing.noteArrival(7)
        now = 200_000L
        timing.noteArrival(7 + VideoReceiver.ARRIVAL_RING)
        now = 300_000L
        assertEquals(200, timing.decodeMicrosFor(7))
        assertEquals(100, timing.decodeMicrosFor(7 + VideoReceiver.ARRIVAL_RING))
        timing.noteArrival(7)
        now = 400_000L
        assertEquals(100, timing.decodeMicrosFor(7))
        timing.noteReleased(7 + VideoReceiver.ARRIVAL_RING)
        now = 500_000L
        assertEquals(300, timing.decodeMicrosFor(7 + VideoReceiver.ARRIVAL_RING))
        assertEquals(-1, timing.decodeMicrosFor(8))
    }

    @Test fun t404_wrappedSequencesAndRepeatedReleasesKeepLatestArrival() {
        var now = 100_000L
        val timing = FrameTiming { now }
        var sequence = Int.MAX_VALUE - 2
        repeat(VideoReceiver.ARRIVAL_RING * 3) {
            timing.noteArrival(sequence)
            now += 10_000
            timing.noteReleased(sequence)
            now += 10_000
            assertEquals(20, timing.decodeMicrosFor(sequence))
            sequence++
        }
        assertEquals(-1, timing.decodeMicrosFor(Int.MAX_VALUE - 2))
        assertTrue(timing.decodeMicrosFor(sequence - VideoReceiver.ARRIVAL_RING) >= 0)
    }

}
