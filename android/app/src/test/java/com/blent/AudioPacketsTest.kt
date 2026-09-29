package com.blent

import org.junit.Assert.*
import org.junit.Test
import okio.Buffer

class AudioPacketsTest {
    private fun packet(sequence: Long, time: Long, direction: Int = 2): ByteArray {
        val buffer = Buffer().writeLong(7).writeLong(sequence).writeLong(time)
            .writeShort(960 * direction).writeByte(direction).writeByte(0)
        repeat(480 * direction) { buffer.writeShortLe(if (it % 2 == 0) 1234 else -4321) }
        return buffer.readByteArray()
    }
    @Test fun t719_frameBoundsReplayGapsAndUnsignedClocks() {
        for (direction in 1..2) {
            val reader = AudioPackets(7, direction)
            val first = reader.read(Buffer().write(packet(0, 1, direction)))
            assertFalse(first.discontinuity)
            assertEquals(480 * direction, first.samples.size)
            assertTrue(first.samples.withIndex().all { (i, v) -> v.toInt() == if (i % 2 == 0) 1234 else -4321 })
            for ((sequence, time) in listOf(0L to 2L, 1L to 1L, -1L to 2L)) {
                try { reader.read(Buffer().write(packet(sequence, time, direction))); fail("invalid sequence/time") }
                catch (_: IllegalArgumentException) {}
            }
            assertTrue(reader.read(Buffer().write(packet(2, Long.MIN_VALUE, direction))).discontinuity)
            assertFalse(reader.read(Buffer().write(packet(3, -1L, direction))).discontinuity)
        }
        val bytes = packet(0, 1)
        for (length in 0 until bytes.size step 13) {
            try { AudioPackets(7, 2).read(Buffer().write(bytes, 0, length)); fail("truncated frame") }
            catch (_: java.io.EOFException) {}
        }
        for (index in (0..15) + (24..27)) {
            val invalid = bytes.copyOf(); invalid[index] = (invalid[index].toInt() xor 0x80).toByte()
            try { AudioPackets(7, 2).read(Buffer().write(invalid)); fail("invalid header byte $index") }
            catch (_: IllegalArgumentException) {}
        }
        for (direction in listOf(-1, 0, 3, Int.MAX_VALUE)) {
            try { AudioPackets(7, direction); fail("invalid direction") } catch (_: IllegalArgumentException) {}
        }
        try { AudioPackets(0, 2); fail("zero generation") } catch (_: IllegalArgumentException) {}
    }
    @Test fun t719_queueBoundsPrefillOverflowDiscontinuitiesAndClock() {
        for (target in -1..210) {
            val valid = target in 20..200 && target % 10 == 0
            try { AudioPlaybackQueue(target) { 0 }; assertTrue(valid) }
            catch (_: IllegalArgumentException) { assertFalse(valid) }
        }
        var now = 0L
        val queue = AudioPlaybackQueue(20) { now }
        assertNull(queue.poll())
        queue.offer(AudioBlock(ShortArray(960) { 1 })); assertNull(queue.poll())
        queue.offer(AudioBlock(ShortArray(960) { 2 })); assertEquals(1, queue.poll()!!.samples[0].toInt())
        assertEquals(2, queue.poll()!!.samples[0].toInt()); assertNull(queue.poll())
        repeat(21) { index -> queue.offer(AudioBlock(ShortArray(960) { index.toShort() })) }
        val afterOverflow = queue.poll()!!
        assertTrue(afterOverflow.discontinuity); assertEquals(1, afterOverflow.samples[0].toInt())
        now = 201; assertNull(queue.poll())
        queue.offer(AudioBlock(ShortArray(960), true)); assertNull(queue.poll())
        queue.offer(AudioBlock(ShortArray(960))); assertTrue(queue.poll()!!.discontinuity)
        queue.clear(); assertNull(queue.poll())
        now = 200
        try { queue.poll(); fail("backwards clock") } catch (_: IllegalArgumentException) {}
        for (size in listOf(0, 1, 959, 961, 1920)) {
            try { queue.offer(AudioBlock(ShortArray(size))); fail("invalid PCM length") } catch (_: IllegalArgumentException) {}
        }
    }
}
