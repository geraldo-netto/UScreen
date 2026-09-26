package com.blent

import android.app.Activity
import android.os.Bundle
import android.os.Debug
import java.io.File
import java.io.InputStream
import java.nio.ByteBuffer
import kotlin.concurrent.thread
import okio.blackholeSink
import okio.buffer
import org.json.JSONArray
import org.json.JSONObject

/** T589: explicit development workload; never packaged in debug/release. */
class AllocationProfileActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.addFlags(android.view.WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        thread(name = "allocation-profile") {
            val result = runCatching { AllocationWorkloads.run() }
                .getOrElse { JSONObject().put("error", it.toString()) }
            File(filesDir, "allocations.json").writeText(result.toString(2))
            runOnUiThread { finish() }
        }
    }
}

internal object AllocationWorkloads {
    fun run(): JSONObject {
        val rows = JSONArray()
        rows.put(display("steady", intArrayOf(64 * 1024), false, false))
        rows.put(display("keyframe", intArrayOf(2 * 1024 * 1024, 64 * 1024), false, false))
        rows.put(display("resize", intArrayOf(64 * 1024, 512 * 1024, 2 * 1024 * 1024), false, false))
        rows.put(display("maximum", intArrayOf(VideoReceiver.MAX_FRAME_SIZE + 1, 64 * 1024), false, false))
        rows.put(display("slow-consumer", intArrayOf(64 * 1024), true, false))
        rows.put(display("reconnect", intArrayOf(64 * 1024), false, true))
        rows.put(camera())
        return JSONObject().put("source", ProfileBuild.SOURCE_ID).put("workloads", rows)
            .put("scope", "Production packet reader and camera packet writer on ART; synthetic payloads; no codec, sensor or display latency measurement")
    }

    private fun settled(): JSONObject {
        // ART allocation counters may be published only at GC boundaries.
        // Keep forced collections separate from collections during the workload.
        System.gc()
        Thread.sleep(150)
        return runtime()
    }

    private fun runtime(): JSONObject = JSONObject().apply { Debug.getRuntimeStats().forEach { (key, value) -> put(key, value) } }

    private fun display(name: String, sizes: IntArray, slow: Boolean, reconnect: Boolean): JSONObject {
        val packets = if (reconnect) 1200 else 300
        val stream = PacketStream(sizes)
        var reader = VideoPacketReader(stream)
        val storage = VideoPacketReader::class.java.getDeclaredField("data").apply { isAccessible = true }
        var maximum = 0
        var growths = 0
        var last = storage.get(reader)
        val before = settled()
        val started = System.nanoTime()
        repeat(packets) {
            if (reconnect) reader = VideoPacketReader(stream)
            check(reader.read())
            val current = storage.get(reader) as ByteArray
            if (current !== last) growths++
            last = current
            maximum = maxOf(maximum, current.size)
            if (slow) Thread.sleep(2)
        }
        val after = runtime()
        val elapsed = System.nanoTime() - started
        val settledAfter = settled()
        return JSONObject().put("name", name).put("packets", packets).put("before", before)
            .put("after", after).put("settled_after", settledAfter).put("elapsed_ns", elapsed)
            .put("storage_replacements", growths).put("maximum_capacity", maximum)
            .put("retained_capacity", (storage.get(reader) as ByteArray).size)
    }

    private fun camera(): JSONObject {
        val bytes = ByteBuffer.allocateDirect(1024 * 1024)
        val sink = blackholeSink().buffer()
        val before = settled()
        val started = System.nanoTime()
        repeat(1200) { CameraWire.packet(sink, bytes, 0, if (it % 30 == 0) bytes.capacity() else 64 * 1024) }
        sink.close()
        val after = runtime()
        val elapsed = System.nanoTime() - started
        val settledAfter = settled()
        return JSONObject().put("name", "camera-packets").put("packets", 1200)
            .put("before", before).put("after", after).put("settled_after", settledAfter).put("elapsed_ns", elapsed)
            .put("payload_bytes", 40 * 1024 * 1024 + 1160 * 64 * 1024)
    }
}

/** Infinite valid length-framed payloads; no per-packet fixture allocation. */
internal class PacketStream(private val sizes: IntArray) : InputStream() {
    private var packet = 0
    private var remaining = 0
    override fun read(): Int = error("Bulk reads required")
    override fun read(bytes: ByteArray, offset: Int, length: Int): Int {
        if (remaining == 0) {
            check(length == 4)
            remaining = sizes[packet++ % sizes.size]
            for (index in 0..3) bytes[offset + index] = (remaining ushr (24 - index * 8)).toByte()
            return 4
        }
        val count = minOf(length, remaining)
        bytes.fill(0, offset, offset + count)
        remaining -= count
        return count
    }
}
