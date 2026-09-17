package com.uscreen

import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.io.DataOutputStream
import java.io.EOFException
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34])
class VideoPacketReaderTest {
    private class Fragmented(bytes: ByteArray, private val fragment: Int) : ByteArrayInputStream(bytes) {
        override fun read(buffer: ByteArray, offset: Int, length: Int): Int =
            super.read(buffer, offset, minOf(length, fragment))
    }
    private class Sink : VideoPacketSink {
        val configs = mutableListOf<ByteArray>()
        val frames = mutableListOf<Pair<Int, ByteArray>>()
        override fun configuration(data: ByteArray, offset: Int, size: Int) {
            configs.add(data.copyOfRange(offset, offset + size))
        }
        override fun frame(sequence: Int, data: ByteArray, offset: Int, size: Int) {
            frames.add(sequence to data.copyOfRange(offset, offset + size))
        }
    }
    private fun packet(type: Int, payload: ByteArray, sequence: Int? = null): ByteArray {
        val bytes = ByteArrayOutputStream()
        DataOutputStream(bytes).use {
            it.writeInt(1 + payload.size + if (sequence == null) 0 else 4)
            it.writeByte(type)
            sequence?.let(it::writeInt)
            it.write(payload)
        }
        return bytes.toByteArray()
    }

    @Test fun t376_fragmentedFramingPreservesBorrowedPayloadsAndSequences() {
        val config = byteArrayOf(0, 0, 0, 1, 103, 42)
        val payload = byteArrayOf(0, 0, 1, 101, -1)
        val stream = packet(0, config) + packet(1, payload, -1) + packet(1, payload, 0)
        for (fragment in 1..7) {
            val reader = VideoPacketReader(Fragmented(stream, fragment))
            val sink = Sink()
            repeat(3) { assertTrue(reader.read()); assertTrue(reader.dispatch(sink)) }
            assertArrayEquals(config, sink.configs.single())
            assertEquals(listOf(-1, 0), sink.frames.map { it.first })
            sink.frames.forEach { assertArrayEquals(payload, it.second) }
            assertThrows(EOFException::class.java) { reader.read() }
        }
    }

    @Test fun t376_eofNeverDeliversPartialHeaderOrPayload() {
        val stream = packet(1, byteArrayOf(7, 8, 9), 123)
        for (length in stream.indices) {
            val reader = VideoPacketReader(Fragmented(stream.copyOf(length), 1))
            val sink = Sink()
            assertThrows(EOFException::class.java) {
                if (reader.read()) reader.dispatch(sink)
            }
            assertTrue(sink.frames.isEmpty())
        }
    }

    @Test fun t376_rejectsInvalidLengthTypeAndTruncatedSequence() {
        for (length in listOf(-1, 0, 1, VideoReceiver.MAX_FRAME_SIZE + 2)) {
            val bytes = ByteArrayOutputStream()
            DataOutputStream(bytes).writeInt(length)
            assertFalse(VideoPacketReader(bytes.toByteArray().inputStream()).read())
        }
        for (bytes in listOf(packet(9, byteArrayOf(42)), packet(1, byteArrayOf(0, 0, 0, 1)))) {
            val reader = VideoPacketReader(bytes.inputStream())
            val sink = Sink()
            assertTrue(reader.read())
            assertFalse(reader.dispatch(sink))
            assertTrue(sink.frames.isEmpty())
            assertTrue(sink.configs.isEmpty())
        }
    }

    @Test fun t376_grownStorageRetainsExactLengthForFollowingSmallFrame() {
        val large = ByteArray(600_000) { (it % 251).toByte() }
        val small = byteArrayOf(17, 42)
        val reader = VideoPacketReader((packet(1, large, 7) + packet(1, small, 8)).inputStream())
        val sink = Sink()
        repeat(2) { assertTrue(reader.read()); assertTrue(reader.dispatch(sink)) }
        assertArrayEquals(large, sink.frames[0].second)
        assertArrayEquals(small, sink.frames[1].second)
    }

    @Test fun t376_timingBoundaryUsesInjectedMonotonicClock() {
        var now = 1_000_000_000L
        val timing = FrameTiming { now }
        timing.noteArrival(9)
        now += 300_000
        timing.noteReleased(9)
        now += 500_000
        assertEquals(800, timing.decodeMicrosFor(9))
        assertEquals(-1, timing.decodeMicrosFor(10))
    }
}
