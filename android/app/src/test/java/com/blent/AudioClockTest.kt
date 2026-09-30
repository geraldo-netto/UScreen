package com.blent

import java.nio.ByteBuffer
import okio.Buffer
import org.junit.Assert.*
import org.junit.Test

class AudioClockTest {
    private fun stamp(frames: Long = 0, nanos: Long = 1, epoch: Long = 1) = AudioClockSample(epoch, frames, nanos)
    private fun reject(action: () -> Unit) { assertThrows(IllegalArgumentException::class.java, action) }
    @Test fun t720_counterCodecRejectsInvalidAndBoundedTruncations() {
        assertNull(AudioClockSample.read(ByteArray(24)))
        for (sample in listOf(stamp(), stamp(Long.MAX_VALUE, Long.MAX_VALUE, Long.MAX_VALUE))) {
            assertEquals(sample, AudioClockSample.read(sample.bytes()))
        }
        for (size in 0..64) if (size != 24) reject { AudioClockSample.read(ByteArray(size)) }
        for (sample in listOf(stamp(-1), stamp(nanos = 0), stamp(nanos = -1), stamp(epoch = 0), stamp(epoch = -1))) {
            assertFalse(sample.valid()); reject { sample.bytes() }
            reject { AudioClockSample.read(ByteBuffer.allocate(24).putLong(sample.epoch).putLong(sample.frames).putLong(sample.nanos).array()) }
        }
    }
    @Test fun t720_windowsNeedNativeProgressAndExpireDespiteRepeatedOrTinyUpdates() {
        val window = AudioClockWindow()
        assertFalse(window.observe(null, 0)); assertFalse(window.observe(stamp(-1), 0))
        assertFalse(window.observe(stamp(), 0)); assertFalse(window.observe(stamp(), 1))
        assertNull(window.rate(1))
        assertFalse(window.observe(stamp(96000, 2_000_000_001), 2000))
        assertEquals(96000L to 2_000_000_000L, window.rate(2000))
        assertNull(window.rate(1999)); assertNull(window.rate(5001))
        assertFalse(window.observe(stamp(96001, 2_000_000_002), 6000)); assertNull(window.rate(6000))
        for (bad in listOf(stamp(1, 3_000_000_001, 2), stamp(1, 3_000_000_001), stamp(96000, 1))) {
            window.clear(); window.observe(stamp(96000, 2_000_000_001), 0)
            assertTrue(window.observe(bad, 1)); assertNull(window.rate(1))
        }
        for (bad in listOf(stamp(0, 2_000_000_001), stamp(480001, 2_000_000_001), stamp(96000, 5_000_000_002))) {
            window.clear(); window.observe(stamp(), 0)
            assertTrue(window.observe(bad, 5000)); assertNull(window.rate(5000))
        }
    }
    @Test fun t720_normalizedRatesRespectExactPositiveAndNegativeBounds() {
        for (ppm in listOf(-1001, -1000, -500, 0, 500, 1000, 1001)) {
            val drift = AudioClockDrift()
            assertNull(drift.correction(0))
            drift.source.observe(stamp(), 0)
            drift.source.observe(stamp(192000 + 192000L * ppm / 1_000_000, 4_000_000_001), 4000)
            assertNull(drift.correction(4000))
            drift.destination.observe(stamp(nanos = 900_000_000_001), 0)
            drift.destination.observe(stamp(192000, 904_000_000_001), 4000)
            // Rounding native frame counters is expected; compare the exact rational value.
            val actual = (192000L * ppm / 1_000_000) * 1_000_000 / 192000
            assertEquals(actual.toInt(), drift.correction(4000))
            drift.clear(); assertNull(drift.correction(4000))
        }
        for (extra in listOf(0L, 1L)) {
            val drift = AudioClockDrift()
            drift.source.observe(stamp(), 0); drift.destination.observe(stamp(), 0)
            drift.source.observe(stamp(96096, 2_000_000_001), 2000)
            drift.destination.observe(stamp(96000, 2_000_000_001 + extra), 2000)
            if (extra == 0L) assertEquals(1000, drift.correction(2000)) else reject { drift.correction(2000) }
        }
    }
    @Test fun t720_queueInterpolatesStereoAtBothRateSignsAndClearsOnCounterReset() {
        for (ppm in listOf(-1000, -500, 0, 500, 1000)) {
            var now = 0L
            val queue = AudioPlaybackQueue(40) { now }
            assertTrue(queue.driftDescription().contains("unmeasured"))
            for (tick in 0L..600) {
                now = tick * 10
                val clock = stamp(tick * 480 + tick * 480 * ppm / 1_000_000, 1_000_000_001 + now * 1_000_000)
                queue.offer(AudioBlock(ShortArray(960) { if (it % 2 == 0) 1234 else -4321 }, clock = clock))
                queue.nativeClock(stamp(tick * 480, 900_000_000_001 + now * 1_000_000, 8))
                queue.poll()?.samples?.toList()?.chunked(2)?.forEach { assertEquals(listOf<Short>(1234, -4321), it) }
            }
            assertEquals("Native drift correction: $ppm ppm", queue.driftDescription())
            queue.nativeClock(null); assertTrue(queue.driftDescription().contains("unmeasured"))
            queue.nativeClock(stamp(epoch = 8)); queue.nativeClock(stamp(nanos = 2, epoch = 9))
            assertNull(queue.poll())
            queue.offer(AudioBlock(ShortArray(960), clock = stamp(epoch = 2)))
            queue.offer(AudioBlock(ShortArray(960), clock = stamp(nanos = 2, epoch = 3)))
            assertNull(queue.poll())
        }
    }
    @Test fun t720_queueRejectsExcessRateAndBoundsInterpolationAcrossBlocks() {
        var now = 0L
        val queue = AudioPlaybackQueue(20) { now }
        queue.offer(AudioBlock(ShortArray(960), clock = stamp())); queue.nativeClock(stamp())
        now = 2000
        queue.offer(AudioBlock(ShortArray(960), clock = stamp(97000, 2_000_000_001)))
        queue.nativeClock(stamp(96000, 2_000_000_001))
        assertNull(queue.poll()); assertTrue(queue.driftDescription().contains("unmeasured"))
        // Seeded, bounded amplitude/phase exercise retains stereo and never indexes beyond the queue.
        val random = java.util.Random(720)
        for (tick in 0L..1000) {
            now = 2000 + tick * 10
            queue.offer(AudioBlock(ShortArray(960) { random.nextInt(65536).toShort() },
                clock = stamp(tick * 480 + tick * 48 / 100, 1 + tick * 10_000_000)))
            queue.nativeClock(stamp(tick * 480, 1 + tick * 10_000_000))
            if (tick % 17 != 0L) queue.poll()?.let { assertEquals(960, it.samples.size) }
        }
        now += 201; assertNull(queue.poll())
        now--; reject { queue.poll() }
    }
    private fun grant(endpoint: AudioEndpoint, magic: String): ByteArray = ByteBuffer.allocate(92)
        .put(magic.toByteArray()).put("b".repeat(64).toByteArray()).putLong(1)
        .put(endpoint.direction.toByte()).put(endpoint.direction.toByte()).putInt(48000).putShort(480)
        .put(endpoint.processing.toByte()).put(if (endpoint.background) 1 else 0).putShort(endpoint.bufferMs.toShort()).array()
    @Test fun t720_wireNegotiatesClockMetadataAndRejectsInvalidPacketsAtomically() {
        for (direction in 1..2) {
            val endpoint = AudioEndpoint("a".repeat(64), 12345, direction, 1, 40, false)
            val wire = AudioWire(endpoint, true); val request = Buffer()
            wire.request(request, 7, true); assertEquals("BLAUREQ2", request.readUtf8(8))
            wire.negotiate(Buffer().writeUtf8(endpoint.token).write(grant(endpoint, "BLAUD002")))
            val packet = Buffer(); val clock = stamp(96000, 2_000_000_001)
            reject { wire.send(packet, ShortArray(480 * direction), 1, 100, stamp(epoch = 0)) }; assertEquals(0, packet.size)
            wire.send(packet, ShortArray(480 * direction) { 123 }, 1, 100, clock)
            val bytes = packet.readByteArray(); assertEquals(28 + 24 + 960 * direction, bytes.size)
            val receiver = AudioPackets(1, direction, true)
            for (size in bytes.indices) {
                assertThrows(java.io.EOFException::class.java) { receiver.read(Buffer().write(bytes.copyOf(size))) }
            }
            val bad = bytes.clone(); bad.fill(0, 28, 36)
            reject { receiver.read(Buffer().write(bad)) }
            val block = receiver.read(Buffer().write(bytes)); assertEquals(clock, block.clock)
            assertTrue(block.samples.all { it == 123.toShort() })
            reject { receiver.read(Buffer().write(bytes)) }
            wire.send(packet, ShortArray(480 * direction), 2, 100)
            assertNull(wire.receive(Buffer().write(bytes)).let { wire.receive(packet).clock })
            // A legacy grant remains usable; counters are deliberately unavailable.
            val legacy = AudioWire(endpoint, true)
            legacy.negotiate(Buffer().writeUtf8(endpoint.token).write(grant(endpoint, "BLAUD001")))
            legacy.send(packet, ShortArray(480 * direction), 1, 100, clock)
            assertEquals(28L + 960 * direction, packet.size)
        }
    }
}
