package com.blent

import android.app.Activity
import android.os.Bundle
import android.os.Debug
import java.io.File
import java.nio.ByteBuffer
import kotlin.concurrent.thread
import okio.Buffer
import okio.BufferedSink
import okio.Sink
import okio.Timeout
import okio.buffer
import org.json.JSONArray
import org.json.JSONObject

/** T607 development-only sweep; no camera, encoder or microphone is opened. */
class CameraChunkProfileActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.addFlags(android.view.WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        thread(name = "camera-chunk-profile") {
            val port = intent.getIntExtra("port", 0)
            val result = runCatching {
                if (port == 0) CameraChunkWorkloads.run() else CameraSocketWorkloads.run(filesDir, port)
            }
                .getOrElse { JSONObject().put("error", it.toString()) }
            File(filesDir, "camera-chunks.json").writeText(result.toString(2))
            runOnUiThread { finish() }
        }
    }
}

internal object CameraChunkWorkloads {
    private const val MAX_PACKET = 2 * 1024 * 1024
    private val chunks = intArrayOf(512, 1024, 2048, 4096, 8192, 12288, 16384, 32768, 65536)

    fun run(): JSONObject {
        val bytes = ByteBuffer.allocateDirect(1024 * 1024)
        repeat(bytes.capacity()) { bytes.put(it, (it * 31).toByte()) }
        val checks = validate(bytes)
        // Warm every candidate before measurement; rotating/reversed order limits drift.
        chunks.forEach { measure(bytes, it, -1, 120) }
        val rows = JSONArray()
        repeat(8) { trial ->
            val order = chunks.indices.map { chunks[(it + trial) % chunks.size] }
            (if (trial % 2 == 0) order else order.reversed()).forEach { rows.put(measure(bytes, it, trial, 1200)) }
        }
        return JSONObject().put("source", ProfileBuild.SOURCE_ID).put("rows", rows)
            .put("identity_checks", checks).put("scope", "ART synthetic sink; sensor off; one flush per packet")
    }

    private fun validate(bytes: ByteBuffer): Int {
        var checks = 0
        for (chunk in chunks) {
            for (size in intArrayOf(1, 511, 8191, 8192, 8193, 65536, bytes.capacity())) {
                val expected = Buffer()
                CameraWire.packet(expected, bytes, 0, size)
                val actual = Buffer()
                packet(actual, bytes, 0, size, chunk)
                check(expected.readByteArray().contentEquals(actual.readByteArray()))
                check(bytes.position() == 0 && bytes.limit() == bytes.capacity())
                checks++
            }
        }
        return checks
    }

    private fun runtime(): JSONObject = JSONObject().apply {
        Debug.getRuntimeStats().forEach { (key, value) -> put(key, value) }
    }

    private fun settled(): JSONObject {
        System.gc()
        Thread.sleep(150)
        return runtime()
    }

    private fun measure(bytes: ByteBuffer, chunk: Int, trial: Int, packets: Int): JSONObject {
        val counter = ChunkCountingSink()
        val sink = counter.buffer()
        val before = settled()
        val cpu = Debug.threadCpuTimeNanos()
        val started = System.nanoTime()
        repeat(packets) { packet(sink, bytes, 0, if (it % 30 == 0) bytes.capacity() else 65536, chunk) }
        val elapsed = System.nanoTime() - started
        val cpuElapsed = Debug.threadCpuTimeNanos() - cpu
        val retained = sink.buffer.size
        sink.close()
        val after = runtime()
        val settledAfter = settled()
        return JSONObject().put("trial", trial).put("chunk", chunk).put("packets", packets)
            .put("elapsed_ns", elapsed).put("cpu_ns", cpuElapsed).put("before", before)
            .put("after", after).put("settled_after", settledAfter).put("write_calls", counter.calls)
            .put("wire_bytes", counter.bytes).put("peak_staging_bytes", counter.peak)
            .put("retained_buffer_bytes", retained)
    }

    // Same bounded packet algorithm as CameraWire.packet, with only the chunk varied.
    // camera-chunks.py checks the source bodies match before running the sweep.
    internal fun packet(sink: BufferedSink, source: ByteBuffer, offset: Int, size: Int, chunk: Int) {
        require(size in 1..MAX_PACKET) { "Invalid camera packet size" }
        require(offset >= 0 && offset <= source.limit() - size) { "Invalid camera buffer bounds" }
        val view = source.duplicate().apply { position(offset); limit(offset + size) }
        sink.writeInt(size)
        val end = offset + size
        while (view.position() < end) {
            view.limit(minOf(end, view.position() + chunk))
            sink.write(view)
        }
        sink.flush()
    }
}

private class ChunkCountingSink : Sink {
    var calls = 0L
    var bytes = 0L
    var peak = 0L
    override fun write(source: Buffer, byteCount: Long) {
        calls++
        bytes += byteCount
        peak = maxOf(peak, source.size)
        source.skip(byteCount)
    }
    override fun flush() = Unit
    override fun close() = Unit
    override fun timeout(): Timeout = Timeout.NONE
}
