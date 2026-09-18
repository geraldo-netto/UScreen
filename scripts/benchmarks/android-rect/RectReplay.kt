package com.uscreen.benchmark

import android.view.Surface
import com.uscreen.BenchMetrics
import java.io.File
import java.nio.ByteBuffer
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.locks.LockSupport
import org.json.JSONArray
import org.json.JSONObject

internal class RectReplay(private val surface: Surface, private val active: AtomicBoolean) {
    private val output = ByteBuffer.allocateDirect(1280 * 800 * 3)
    private var context = 0L
    private val timings = LongArray(60 * 30 * 8)
    private var count = 0
    private var pending = 0
    private var updates = 0

    fun run(file: File, seconds: Int, warmup: Int, verify: Boolean, mapped: Boolean): JSONObject {
        require(seconds in 1..30 && warmup in 0..5)
        BenchMetrics.snapshot() // Same fixed instrumentation backing as H.264 replay.
        context = RectNative.create()
        check(context != 0L)
        try {
            RectClip(file, mapped).use { clip ->
                RectGl(surface).use { renderer ->
                    val readback = if (verify) ByteBuffer.allocateDirect(1280 * 800 * 4) else null
                    phase(clip, renderer, warmup, readback, false)
                    val before = ReplayStats.process()
                    phase(clip, renderer, seconds, readback, true)
                    val after = ReplayStats.process()
                    val deadline = System.nanoTime() + 100_000_000
                    while (active.get() && pending < count && System.nanoTime() < deadline) {
                        presentations(renderer)
                        LockSupport.parkNanos(1_000_000)
                    }
                    return result(clip, renderer, before, after, readback != null)
                }
            }
        } finally { RectNative.destroy(context); context = 0 }
    }
    private fun result(clip: RectClip, renderer: RectGl, before: JSONObject,
                       after: JSONObject, verified: Boolean): JSONObject {
        val rows = JSONArray()
        repeat(count) { index ->
            rows.put(JSONArray().apply { repeat(8) { put(timings[index * 8 + it]) } })
        }
        return JSONObject().put("completed", active.get()).put("mode", "rect")
            .put("codec", clip.codec).put("rate", clip.rate).put("count", count).put("updates", updates)
            .put("verified", verified).put("before", before).put("after", after)
            .put("gpu", renderer.identity()).put("trace", rows)
            .put("trace_columns", JSONArray(listOf("due_ns", "begin_ns", "decoded_ns", "uploaded_ns", "swapped_ns", "bytes", "egl_frame_id", "display_present_ns")))
            .put("egl_display_timestamps", renderer.timestamps)
            .put("boundary", "local fixture to swap return and supported EGL display-present timestamp; no capture/USB/optical timing")
            .put("owned_rgb_scratch_bytes", output.capacity()).put("input_mode", if (clip.mapped) "mmap" else "read")
            .put("compressed_storage_bytes", clip.bytes.capacity())
    }
    private fun phase(clip: RectClip, renderer: RectGl, seconds: Int, verify: ByteBuffer?, record: Boolean) {
        val start = System.nanoTime()
        repeat(clip.rate * seconds) { index ->
            val due = start + index * 1_000_000_000L / clip.rate
            parkUntil(due)
            if (!active.get()) return
            val frame = clip.frames[index % clip.frames.size]
            update(clip, renderer, frame, due, record)
            if (verify != null) renderer.verify(frame.hash, verify)
        }
        parkUntil(start + seconds * 1_000_000_000L)
    }
    private fun update(clip: RectClip, renderer: RectGl, frame: RectFrame, due: Long, record: Boolean) {
        val begin = System.nanoTime()
        if (frame.length > 0) {
            val offset = clip.dataOffset(frame)
            check(RectNative.decode(context, clip.codec, clip.bytes, offset, frame.length, output, frame.rawBytes))
        }
        val decoded = System.nanoTime()
        if (frame.length > 0) renderer.upload(frame, output)
        val uploaded = System.nanoTime()
        val frameId = if (frame.length > 0) renderer.draw() else -4L
        val swapped = System.nanoTime()
        if (record) {
            val offset = count++ * 8
            timings[offset] = due; timings[offset + 1] = begin; timings[offset + 2] = decoded
            timings[offset + 3] = uploaded; timings[offset + 4] = swapped; timings[offset + 5] = frame.length.toLong()
            timings[offset + 6] = frameId
            if (frame.length > 0) updates++
            presentations(renderer)
        }
    }
    private fun presentations(renderer: RectGl) {
        while (pending < count) {
            val offset = pending * 8
            val id = timings[offset + 6]
            val at = if (id >= 0) renderer.presented(id) else id
            if (at == -2L) return // EGL_TIMESTAMP_PENDING_ANDROID; poll without blocking rendering.
            timings[offset + 7] = at
            pending++
        }
    }
    private fun parkUntil(deadline: Long) {
        while (active.get()) {
            val remaining = deadline - System.nanoTime()
            if (remaining <= 0) return
            LockSupport.parkNanos(remaining.coerceAtMost(25_000_000))
        }
    }
}
