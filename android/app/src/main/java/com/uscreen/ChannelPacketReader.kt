package com.uscreen

import java.io.EOFException
import java.io.InterruptedIOException
import java.net.ProtocolException
import java.net.SocketTimeoutException
import java.nio.ByteBuffer
import java.nio.channels.ClosedChannelException
import java.nio.channels.ClosedSelectorException
import java.nio.channels.SelectionKey
import java.nio.channels.Selector
import java.nio.channels.SocketChannel
import java.util.concurrent.TimeUnit

internal data class ChannelPacketHeader(
    override val size: Int, override val configuration: Boolean, override val sequence: Long,
) : DecoderInputInfo

/** Experimental framed input. Only the tiny wire prefix is staged; payload is
 * read into the caller's buffer. Owns its channel/selector, including cancellation.
 * The current VideoReceiver still selects its reusable InputStream/heap path. */
internal class ChannelPacketReader(
    private val channel: SocketChannel,
    timeoutMillis: Long = 10_000,
    private val clock: () -> Long = System::nanoTime,
) : AutoCloseable {
    private val timeout = TimeUnit.MILLISECONDS.toNanos(timeoutMillis)
    private val selector = Selector.open()
    private val prefix = ByteBuffer.allocateDirect(5)
    private var deadline = 0L
    private var pending = 0
    @Volatile private var closed = false

    init {
        try {
            require(timeoutMillis in 1..60_000)
            channel.configureBlocking(false)
            channel.register(selector, SelectionKey.OP_READ)
        } catch (error: Exception) { close(); throw error }
    }

    fun readHeader(): ChannelPacketHeader {
        check(pending == 0) { "Previous payload has not been consumed" }
        deadline = clock() + timeout
        prefix.clear().limit(4)
        readExact(prefix)
        val wireSize = prefix.getInt(0)
        if (wireSize <= 1 || wireSize > VideoReceiver.MAX_FRAME_SIZE + 1) {
            throw ProtocolException("Invalid packet size: $wireSize")
        }
        prefix.clear().limit(minOf(wireSize, 5))
        readExact(prefix)
        prefix.flip()
        val header = parseHeader(wireSize)
        pending = header.size
        return header
    }

    private fun parseHeader(size: Int): ChannelPacketHeader = when (val type = prefix.get().toInt() and 0xff) {
        VideoReceiver.PACKET_TYPE_CONFIG -> ChannelPacketHeader(size - 1, true, 0)
        VideoReceiver.PACKET_TYPE_FRAME -> {
            if (size <= VideoReceiver.FRAME_HEADER_SIZE) throw ProtocolException("Truncated frame: $size")
            ChannelPacketHeader(size - VideoReceiver.FRAME_HEADER_SIZE, false, prefix.int.toLong() and 0xffff_ffffL)
        }
        else -> throw ProtocolException("Unknown packet type: $type")
    }

    /** Advances position to payload length; callers flip only when reading it. */
    fun readPayload(destination: ByteBuffer) {
        check(pending > 0) { "Read a header before its payload" }
        require(destination.capacity() >= pending) { "Payload exceeds destination capacity" }
        destination.clear().limit(pending)
        destination.put(prefix) // Configuration prefix may include up to four payload bytes.
        readExact(destination)
        pending = 0
    }

    private fun readExact(destination: ByteBuffer) {
        while (destination.hasRemaining()) {
            checkReadable()
            when (channel.read(destination)) {
                -1 -> throw EOFException("Stream closed inside packet")
                0 -> awaitReady()
            }
        }
        checkReadable()
    }

    private fun checkReadable() {
        if (closed || !channel.isOpen) throw ClosedChannelException()
        if (Thread.currentThread().isInterrupted) throw InterruptedIOException("Packet read interrupted")
        if (clock() >= deadline) throw SocketTimeoutException("Packet read deadline expired")
    }

    private fun awaitReady() {
        val remaining = deadline - clock()
        if (remaining <= 0) throw SocketTimeoutException("Packet read deadline expired")
        try {
            selector.select((remaining + 999_999) / 1_000_000)
            selector.selectedKeys().clear()
        } catch (_: ClosedSelectorException) { throw ClosedChannelException() }
    }

    override fun close() {
        closed = true
        selector.wakeup()
        try { channel.close() } finally { selector.close() }
    }
}
