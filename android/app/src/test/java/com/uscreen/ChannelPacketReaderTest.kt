package com.uscreen

import java.io.EOFException
import java.net.InetSocketAddress
import java.net.ProtocolException
import java.net.SocketTimeoutException
import java.nio.ByteBuffer
import java.nio.channels.ServerSocketChannel
import java.nio.channels.SocketChannel
import java.util.concurrent.CountDownLatch
import java.util.concurrent.FutureTask
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicLong
import org.junit.Assert.*
import org.junit.Test

internal class PacketSocketPair : AutoCloseable {
    private val listener = ServerSocketChannel.open().apply { bind(InetSocketAddress("127.0.0.1", 0)) }
    val client = SocketChannel.open(listener.localAddress)
    val producer = listener.accept().apply { socket().tcpNoDelay = true }
    fun send(bytes: ByteArray) {
        val buffer = ByteBuffer.wrap(bytes)
        while (buffer.hasRemaining()) producer.write(buffer)
    }
    override fun close() { client.close(); producer.close(); listener.close() }
}

class ChannelPacketReaderTest {
    @Test fun t497_payloadRequiresAnUnconsumedValidatedHeader() {
        PacketSocketPair().use { sockets ->
            ChannelPacketReader(sockets.client).use { reader ->
                assertThrows(IllegalStateException::class.java) { reader.readPayload(ByteBuffer.allocate(4)) }
                sockets.send(packet(0, byteArrayOf(1, 2, 3, 4)))
                assertEquals(ChannelPacketHeader(4, true, 0), reader.readHeader())
                assertArrayEquals(byteArrayOf(1, 2, 3, 4), payload(reader, 4))
                assertThrows(IllegalStateException::class.java) { reader.readPayload(ByteBuffer.allocate(4)) }
            }
        }
    }

    private fun packet(type: Int, bytes: ByteArray, sequence: Int = 0): ByteArray {
        val extra = if (type == VideoReceiver.PACKET_TYPE_FRAME) 5 else 1
        return ByteBuffer.allocate(4 + extra + bytes.size).putInt(extra + bytes.size)
            .put(type.toByte()).apply { if (extra == 5) putInt(sequence) }.put(bytes).array()
    }

    @Test fun t403_fragmented_config_and_frames_preserve_boundaries_and_unsigned_sequence() {
        PacketSocketPair().use { sockets ->
            ChannelPacketReader(sockets.client).use { reader ->
                val config = byteArrayOf(1, 2, 3)
                val frame = ByteArray(97) { it.toByte() }
                val bytes = packet(0, config) + packet(1, frame, -1) + packet(0, byteArrayOf(9))
                val writer = Thread {
                    bytes.forEach { sockets.send(byteArrayOf(it)); Thread.sleep(1) }
                }.apply { start() }
                try {
                    assertEquals(ChannelPacketHeader(3, true, 0), reader.readHeader())
                    assertArrayEquals(config, payload(reader, 3))
                    assertEquals(ChannelPacketHeader(97, false, 0xffff_ffffL), reader.readHeader())
                    assertArrayEquals(frame, payload(reader, 97))
                    assertEquals(ChannelPacketHeader(1, true, 0), reader.readHeader())
                    assertArrayEquals(byteArrayOf(9), payload(reader, 1))
                } finally { writer.join(2_000) }
                assertFalse(writer.isAlive)
            }
        }
    }

    private fun payload(reader: ChannelPacketReader, size: Int): ByteArray {
        val buffer = ByteBuffer.allocateDirect(size + 16)
        reader.readPayload(buffer)
        buffer.flip()
        assertEquals(size, buffer.remaining())
        return ByteArray(size).also { buffer.get(it) }
    }

    @Test fun t403_rejects_invalid_lengths_types_and_truncated_metadata() {
        val invalid = listOf(
            ByteBuffer.allocate(4).putInt(0).array(),
            ByteBuffer.allocate(4).putInt(-1).array(),
            ByteBuffer.allocate(4).putInt(1).array(),
            ByteBuffer.allocate(4).putInt(VideoReceiver.MAX_FRAME_SIZE + 2).array(),
            byteArrayOf(0, 0, 0, 2, 7, 9),
            byteArrayOf(0, 0, 0, 5, 1, 0, 0, 0, 0),
        )
        invalid.forEach { bytes ->
            PacketSocketPair().use { sockets ->
                ChannelPacketReader(sockets.client).use { reader ->
                    sockets.send(bytes)
                    assertThrows(ProtocolException::class.java) { reader.readHeader() }
                }
            }
        }
    }

    @Test fun t403_eof_cannot_complete_partial_header_or_payload() {
        for (count in 0..3) {
            PacketSocketPair().use { sockets ->
                ChannelPacketReader(sockets.client).use { reader ->
                    sockets.send(byteArrayOf(0, 0, 0, 9).copyOf(count))
                    sockets.producer.close()
                    assertThrows(EOFException::class.java) { reader.readHeader() }
                }
            }
        }
        PacketSocketPair().use { sockets ->
            ChannelPacketReader(sockets.client).use { reader ->
                sockets.send(byteArrayOf(0, 0, 0, 9, 1, 0, 0, 0, 3, 7))
                sockets.producer.close()
                assertEquals(ChannelPacketHeader(4, false, 3), reader.readHeader())
                assertThrows(EOFException::class.java) { reader.readPayload(ByteBuffer.allocateDirect(4)) }
            }
        }
    }

    @Test fun t403_capacity_rejection_consumes_no_payload_and_prevents_header_overtake() {
        PacketSocketPair().use { sockets ->
            ChannelPacketReader(sockets.client).use { reader ->
                sockets.send(packet(1, byteArrayOf(1, 2, 3, 4), Int.MIN_VALUE))
                assertEquals(0x8000_0000L, reader.readHeader().sequence)
                assertThrows(IllegalStateException::class.java) { reader.readHeader() }
                assertThrows(IllegalArgumentException::class.java) { reader.readPayload(ByteBuffer.allocateDirect(3)) }
                assertArrayEquals(byteArrayOf(1, 2, 3, 4), payload(reader, 4))
            }
        }
    }

    @Test fun t403_close_wakes_a_read_without_waiting_for_its_ten_second_deadline() {
        PacketSocketPair().use { sockets ->
            val reader = ChannelPacketReader(sockets.client)
            val entered = CountDownLatch(1)
            val result = FutureTask {
                entered.countDown()
                try { reader.readHeader(); false } catch (_: java.io.IOException) { true }
            }
            val thread = Thread(result).apply { start() }
            assertTrue(entered.await(1, TimeUnit.SECONDS))
            reader.close()
            assertTrue(result.get(500, TimeUnit.MILLISECONDS))
            thread.join(500)
            reader.close()
        }
    }

    @Test fun t403_packet_deadline_does_not_restart_after_partial_progress() {
        PacketSocketPair().use { sockets ->
            val now = AtomicLong()
            ChannelPacketReader(sockets.client, 100, now::get).use { reader ->
                sockets.send(packet(1, byteArrayOf(1, 2, 3, 4)).copyOf(9))
                reader.readHeader()
                now.set(TimeUnit.MILLISECONDS.toNanos(101))
                sockets.send(byteArrayOf(1, 2, 3, 4))
                assertThrows(SocketTimeoutException::class.java) { reader.readPayload(ByteBuffer.allocateDirect(4)) }
            }
        }
    }
}
