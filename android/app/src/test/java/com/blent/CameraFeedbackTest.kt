package com.blent

import java.io.ByteArrayInputStream
import java.io.DataInputStream
import java.nio.ByteBuffer
import okio.Buffer
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34])
class CameraFeedbackTest {
    @Test fun t614_feedbackRequiresExactConnectionLocalSequence() {
        val output = Buffer()
        val acks = Buffer().apply { for (i in 1L..31L) writeLong(i) }.readByteArray()
        val link = CameraLink(output, Buffer().write(acks))
        repeat(31) { assertTrue(link.send(ByteBuffer.wrap(byteArrayOf(7)), 0, 1, 0) >= 0) }
        repeat(31) { assertEquals(1, output.readInt()); assertEquals(7, output.readByte().toInt()) }
        for (ack in listOf(-1L, 0L, 2L, Long.MAX_VALUE)) {
            val input = Buffer().writeLong(ack).readByteArray()
            val invalid = CameraLink(Buffer(), Buffer().write(input))
            assertTrue(runCatching { invalid.send(ByteBuffer.wrap(byteArrayOf(1)), 0, 1, 0) }.isFailure)
        }
        for (length in 0..7) {
            val truncated = CameraLink(Buffer(), Buffer().write(ByteArray(length)))
            assertTrue(runCatching { truncated.send(ByteBuffer.wrap(byteArrayOf(1)), 0, 1, 0) }.isFailure)
        }
    }

    @Test fun t614_relativeAgeExcludesClockOffsetAndRejectsRegression() {
        val clock = CameraFrameClock()
        assertEquals(0L, clock.ageUs(1000, 9_000_000))
        assertEquals(150_000L, clock.ageUs(2000, 9_151_000))
        assertEquals(0L, clock.ageUs(300_000, 9_200_000))
        assertTrue(runCatching { clock.ageUs(2000, 9_300_000) }.isFailure)
        assertTrue(runCatching { clock.ageUs(400_000, 8_000_000) }.isFailure)
        assertTrue(runCatching { CameraFrameClock().ageUs(-1, 0) }.isFailure)
        assertTrue(runCatching { CameraFrameClock().ageUs(0, -1) }.isFailure)
        assertEquals(0L, CameraFrameClock().ageUs(Long.MAX_VALUE, Long.MAX_VALUE))
    }
}
