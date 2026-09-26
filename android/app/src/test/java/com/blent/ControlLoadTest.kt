package com.blent

import android.os.SystemClock
import android.view.MotionEvent
import okhttp3.*
import okio.ByteString
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/** T387: replayable JVM workload, not an ART or network throughput benchmark. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class ControlLoadTest {
    private class Socket(val drain: Boolean, val listener: WebSocketListener) : WebSocket {
        var queued = 0L
        var accepted = 0L
        var cancelled = false
        override fun request() = Request.Builder().url(TouchCapture.WS_URL).build()
        override fun queueSize() = queued
        override fun send(text: String): Boolean {
            val bytes = text.toByteArray(Charsets.UTF_8).size
            if (queued + bytes > 8192) return false
            accepted++
            queued = if (drain) 0 else queued + bytes
            return true
        }
        override fun send(bytes: ByteString) = false
        override fun close(code: Int, reason: String?) = true
        override fun cancel() { cancelled = true }
        fun open() {
            listener.onOpen(this, Response.Builder().request(request()).protocol(Protocol.HTTP_1_1)
                .code(101).message("Switching Protocols").build())
        }
    }

    private fun stroke(): MotionEvent {
        val now = SystemClock.uptimeMillis()
        val properties = arrayOf(MotionEvent.PointerProperties().apply {
            id = 0; toolType = MotionEvent.TOOL_TYPE_STYLUS
        })
        val coords = arrayOf(MotionEvent.PointerCoords().apply { x = 25f; y = 50f; pressure = 0.5f })
        val event = MotionEvent.obtain(now - 12, now - 12, MotionEvent.ACTION_MOVE, 1,
            properties, coords, 0, 0, 1f, 1f, 0, 0, 0, 0)
        for (offset in listOf(8L, 4L, 0L)) {
            coords[0].x += 10f
            event.addBatch(now - offset, coords, 0)
        }
        return event
    }

    private fun replay(capture: TouchCapture) {
        for (index in 0 until 4800) {
            if (!capture.isControlConnected()) break
            val event = stroke()
            try { capture.handleMotionEvent(event, 100, 100) } finally { event.recycle() }
            if (index % 4 == 0) capture.sendRendered(index, 1000)
        }
    }

    // Android's test compiler exposes the Android boot classpath, so JVM-only
    // measurement APIs are accessed reflectively. They are never shipped in the APK.
    private fun gcMetric(method: String): Long {
        val factory = Class.forName("java.lang.management.ManagementFactory")
        val collectors = factory.getMethod("getGarbageCollectorMXBeans").invoke(null) as List<*>
        val getter = Class.forName("java.lang.management.GarbageCollectorMXBean").getMethod(method)
        return collectors.sumOf { (getter.invoke(it) as Long).coerceAtLeast(0) }
    }
    private fun gcCount() = gcMetric("getCollectionCount")
    private fun gcMs() = gcMetric("getCollectionTime")
    private fun allocatedBytes(): Long {
        val factory = Class.forName("java.lang.management.ManagementFactory")
        val bean = factory.getMethod("getThreadMXBean").invoke(null)
        val getter = Class.forName("com.sun.management.ThreadMXBean")
            .getMethod("getThreadAllocatedBytes", Long::class.javaPrimitiveType)
        return getter.invoke(bean, Thread.currentThread().id) as Long
    }

    private fun measure(drain: Boolean) {
        lateinit var socket: Socket
        val capture = TouchCapture(WebSocket.Factory { _, listener -> Socket(drain, listener).also { socket = it } })
        capture.token = "a".repeat(64)
        capture.connect()
        socket.open()
        val count = gcCount()
        val gcTime = gcMs()
        val bytes = allocatedBytes()
        val start = System.nanoTime()
        try {
            replay(capture)
            val elapsed = System.nanoTime() - start
            val allocated = allocatedBytes() - bytes
            val stats = capture.controlStatistics()
            assertEquals(socket.accepted, stats.accepted)
            assertTrue(stats.sampledEvents > 0)
            assertTrue(stats.oldestSampleMs >= 12)
            if (drain) assertDrained(stats) else assertRefused(stats, socket)
            println("T387_RESULT " + JSONObject().apply {
                put("schema", 1); put("runtime", "Robolectric/JVM API34")
                put("scenario", if (drain) "immediate-drain" else "stalled-8KiB")
                put("elapsed_ns", elapsed); put("thread_allocated_bytes", allocated)
                put("jvm_gc_count", gcCount() - count); put("jvm_gc_ms", gcMs() - gcTime)
                put("accepted", stats.accepted); put("rejected", stats.rejected)
                put("peak_queue_bytes", stats.peakQueueBytes)
                put("sample_count", stats.sampledEvents); put("sample_age_total_ms", stats.totalSampleAgeMs)
                put("sample_age_max_ms", stats.oldestSampleMs)
            })
        } finally { capture.disconnect() }
    }

    private fun assertDrained(stats: ControlStatisticsSnapshot) {
        assertEquals(20401L, stats.accepted) // auth + 4 samples/event + ACK every fourth event
        assertEquals(0L, stats.rejected)
        assertEquals(0L, stats.queueBytes)
    }

    private fun assertRefused(stats: ControlStatisticsSnapshot, socket: Socket) {
        assertEquals(1L, stats.rejected)
        assertTrue(socket.cancelled)
        assertTrue(stats.peakQueueBytes <= 8192)
        assertEquals(0L, stats.queueBytes)
    }

    @Test fun t387_replayWithDrainingAndStalledQueues() {
        measure(true)
        measure(false)
    }
}
