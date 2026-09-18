package com.uscreen.benchmark

import com.uscreen.*
import java.net.InetSocketAddress
import java.net.SocketTimeoutException
import java.io.InterruptedIOException
import java.nio.ByteBuffer
import java.nio.channels.ClosedChannelException
import java.nio.channels.ServerSocketChannel
import java.nio.channels.SocketChannel
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicReference
import java.util.concurrent.locks.LockSupport
import org.json.JSONObject

/** Both peers are in this benchmark process. This isolates input transport;
 * it is not a USB/Wi-Fi or end-to-end capture measurement. */
internal class SocketReplay(
    private val decoder: DecoderSession, direct: Boolean,
    private val active: () -> Boolean = { true }, private val writeTimeoutMillis: Long = 10_000,
) : ReplayInput {
    private val listener = ServerSocketChannel.open().apply { bind(InetSocketAddress("127.0.0.1", 0)) }
    private val consumer = SocketChannel.open(listener.localAddress).apply {
        socket().tcpNoDelay = true
        socket().soTimeout = 10_000
        socket().receiveBufferSize = 128 * 1024
    }
    private val producer = listener.accept().apply { socket().tcpNoDelay = true }
    private val receiveBufferBytes = consumer.socket().receiveBufferSize
    private val prefix = ByteBuffer.allocateDirect(9)
    private val channelReader = if (direct) ChannelPacketReader(consumer) else null
    private val packets = AtomicInteger()
    private val directSlots = AtomicInteger()
    private val error = AtomicReference<String?>()
    @Volatile private var closed = false
    private val worker = Thread({ receive() }, "decoder-bench-socket")

    init { listener.close(); producer.configureBlocking(false); worker.start() }

    override fun feed(bytes: ByteArray, configuration: Boolean, sequence: Long) {
        prefix.clear()
        prefix.putInt(bytes.size + if (configuration) 1 else 5).put(if (configuration) 0 else 1)
        if (!configuration) prefix.putInt(sequence.toInt())
        prefix.flip()
        val buffers = arrayOf(prefix, ByteBuffer.wrap(bytes))
        val deadline = System.nanoTime() + writeTimeoutMillis * 1_000_000
        while (buffers.any { it.hasRemaining() }) {
            checkWritable(deadline)
            if (producer.write(buffers) == 0L) LockSupport.parkNanos(10_000_000)
        }
    }

    private fun checkWritable(deadline: Long) {
        if (closed) throw ClosedChannelException()
        if (!active() || Thread.currentThread().isInterrupted) throw InterruptedIOException("Replay input cancelled")
        if (System.nanoTime() >= deadline) throw SocketTimeoutException("Replay write deadline expired")
    }

    private fun receive() {
        try {
            val direct = channelReader
            if (direct != null) receiveDirect(direct) else receiveHeap()
        } catch (failure: Exception) {
            if (!closed) {
                error.set(failure.toString())
                decoder.resetCodec()
            }
        } finally { close() }
    }

    private fun receiveDirect(reader: ChannelPacketReader) {
        while (!closed) {
            val info = reader.readHeader()
            val codec = decoder.mediaCodec ?: return
            if (!decoder.feedDirect(codec, info) { buffer ->
                    if (buffer.isDirect) directSlots.incrementAndGet()
                    reader.readPayload(buffer)
                }) return
            packets.incrementAndGet()
        }
    }

    private fun receiveHeap() {
        val reader = VideoPacketReader(consumer.socket().getInputStream())
        val sink = object : VideoPacketSink {
            override fun configuration(data: ByteArray, offset: Int, size: Int) = submit(data, offset, size, true, 0)
            override fun frame(sequence: Int, data: ByteArray, offset: Int, size: Int) =
                submit(data, offset, size, false, sequence.toLong() and 0xffff_ffffL)
        }
        while (!closed && reader.read()) {
            if (!reader.dispatch(sink) || decoder.mediaCodec == null) return
            packets.incrementAndGet()
        }
    }

    private fun submit(bytes: ByteArray, offset: Int, size: Int, configuration: Boolean, sequence: Long) {
        val codec = decoder.mediaCodec ?: return
        decoder.feedDecoder(codec, bytes, offset, size, configuration, sequence)
    }

    override fun summary(): JSONObject = JSONObject().put("kind", if (channelReader == null) "heap" else "direct")
        .put("packets", packets.get()).put("direct_slots", directSlots.get())
        .put("receive_buffer_bytes", receiveBufferBytes).put("error", error.get() ?: JSONObject.NULL)

    override fun close() {
        closed = true
        channelReader?.close()
        consumer.close()
        producer.close()
        if (Thread.currentThread() !== worker) worker.join(500)
    }
}
