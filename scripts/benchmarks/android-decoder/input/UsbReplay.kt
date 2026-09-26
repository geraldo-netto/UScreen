package com.blent.benchmark

import android.view.Surface
import com.blent.*
import java.io.DataInputStream
import java.io.DataOutputStream
import java.net.InetSocketAddress
import java.net.Socket
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.locks.LockSupport
import org.json.JSONObject

/** Isolated raw-write/encode/USB/decoder experiment, not an EVDI display. */
internal class UsbReplay(private val surface: Surface, private val active: AtomicBoolean) {
    private val stats = ReplayStats()
    private val messages = ArrayBlockingQueue<(DataOutputStream) -> Unit>(512)
    private val pending = AtomicInteger()
    private val controlledRestart = AtomicBoolean()
    private val decoder = DecoderSession(Any(), active::get, FrameTiming(), object : DecoderEvents {
        override fun rendered(sequence: Int, decodeMicros: Int) {
            // Publish the ACK obligation before exposing render completion (T571).
            send { it.writeByte(1); it.writeInt(sequence); it.writeInt(decodeMicros) }
            stats.rendered(sequence, decodeMicros)
        }
        override fun invalidated() {
            stats.invalidated()
            if (!controlledRestart.get()) active.set(false)
        }
    }, { ReceiverStatistics() })
    private var count = 0
    private var lastSequence = 0

    fun run(port: Int): JSONObject {
        require(port in 1024..65535)
        val socket = Socket()
        var writer: Thread? = null
        try {
            socket.connect(InetSocketAddress("127.0.0.1", port), 3000)
            socket.tcpNoDelay = true
            socket.soTimeout = 3000
            val input = DataInputStream(socket.getInputStream().buffered())
            writer = writer(DataOutputStream(socket.getOutputStream()))
            val metadata = JSONObject(input.readUTF())
            val format = format(metadata)
            stats.begin(1)
            setup(format)
            while (active.get() && packet(input, format)) {}
            val deadline = System.nanoTime() + 750_000_000L
            while (active.get() && stats.count() < count && System.nanoTime() < deadline) LockSupport.parkNanos(1_000_000)
            while (active.get() && pending.get() > 0 && System.nanoTime() < deadline) LockSupport.parkNanos(1_000_000)
            return JSONObject().put("completed", completed()).put("sent", count).put("stats", stats.finish())
                .put("metadata", metadata).put("selection_receipt", replayReceipt(decoder))
        } finally {
            socket.close()
            active.set(false)
            writer?.join(1000)
            decoder.releaseCodec()
        }
    }

    private fun completed(): Boolean = active.get() && stats.count() == count && pending.get() == 0

    private fun format(metadata: JSONObject): DecoderFormat {
        val width = metadata.getInt("width")
        val height = metadata.getInt("height")
        val fps = metadata.getInt("fps")
        require(width in 2..4096 && height in 2..4096 && fps in 10..90)
        require(metadata.getString("mime") == "video/avc")
        return requestFormat("video/avc", width, height, fps, metadata.getJSONObject("selection"))
    }

    private fun setup(format: DecoderFormat) {
        val started = System.nanoTime()
        check(decoder.setupCodec(surface, format))
        val receipt = replayReceipt(decoder).orEmpty()
        val duration = (System.nanoTime() - started) / 1000
        send { it.writeByte(0); it.writeUTF(receipt); it.writeLong(duration) }
    }

    private fun packet(input: DataInputStream, format: DecoderFormat): Boolean {
        val kind = input.readInt()
        if (kind == 2) return false
        if (kind == 1) { restart(format); return true }
        require(kind == 0)
        val sequence = input.readInt()
        val size = input.readInt()
        require(sequence > lastSequence && size in 1..(8 * 1024 * 1024))
        lastSequence = sequence
        val bytes = ByteArray(size).also(input::readFully)
        val codec = checkNotNull(decoder.mediaCodec)
        decoder.feedDecoder(codec, bytes, 0, bytes.size, false, sequence.toLong())
        count++
        return true
    }

    private fun restart(format: DecoderFormat) {
        controlledRestart.set(true)
        try { decoder.resetCodec() } finally { controlledRestart.set(false) }
        setup(format)
    }

    private fun send(message: (DataOutputStream) -> Unit) {
        pending.incrementAndGet()
        if (!messages.offer(message)) active.set(false)
    }

    private fun writer(output: DataOutputStream) = Thread({
        try {
            while (active.get()) {
                messages.poll(100, TimeUnit.MILLISECONDS)?.let {
                    it(output)
                    output.flush()
                    pending.decrementAndGet()
                }
            }
        } catch (_: Exception) { active.set(false) }
    }, "decoder-bench-acks").apply { start() }
}
