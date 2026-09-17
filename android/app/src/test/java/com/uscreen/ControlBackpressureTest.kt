package com.uscreen

import android.view.MotionEvent
import okhttp3.*
import okio.ByteString
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34])
class ControlBackpressureTest {
    private class Socket(val listener: WebSocketListener) : WebSocket {
        val attempted = mutableListOf<JSONObject>()
        val accepted = mutableListOf<JSONObject>()
        var rejectType: String? = null
        var cancelled = false
        var queued = 0L
        override fun request() = Request.Builder().url(TouchCapture.WS_URL).build()
        override fun queueSize() = queued
        override fun send(text: String): Boolean {
            val message = JSONObject(text)
            attempted.add(message)
            if (message.getString("type") == rejectType) return false
            accepted.add(message)
            queued += text.toByteArray(Charsets.UTF_8).size
            return true
        }
        override fun send(bytes: ByteString) = false
        override fun close(code: Int, reason: String?) = true
        override fun cancel() { cancelled = true }
        fun open() {
            listener.onOpen(this, Response.Builder().request(request()).protocol(Protocol.HTTP_1_1)
                .code(101).message("Switching Protocols").build())
        }
        fun greet() { listener.onMessage(this, """{"status":"connected"}""") }
        fun types() = accepted.map { it.getString("type") }
    }

    private class Connection {
        val sockets = mutableListOf<Socket>()
        val created = java.util.concurrent.LinkedBlockingQueue<Socket>()
        val capture = TouchCapture(WebSocket.Factory { _, listener ->
            Socket(listener).also { sockets.add(it); created.add(it) }
        }).apply { token = "a".repeat(64); setNativeResolution(100, 200) }
        fun connect(): Socket { capture.connect(); return sockets.last() }
        fun recover(): Socket = connect().also { it.open(); it.greet() }
    }

    @Test fun t387_rejectedHandshakeStopsAndRetainsPendingSettings() {
        for (kind in listOf("auth", "resolution", "config", "mode")) {
            val connection = Connection()
            val capture = connection.capture
            capture.sendConfig(2000, 30)
            capture.sendMode(true)
            try {
                val socket = connection.connect()
                socket.rejectType = kind
                socket.open()
                assertFalse("T387 rejected $kind left connection usable", capture.isControlConnected())
                assertTrue(socket.cancelled)
                assertEquals(kind, socket.attempted.last().getString("type"))
                socket.greet()
                assertFalse(capture.controlConnected.value)
                val next = connection.recover()
                assertEquals(listOf("auth", "resolution", "config", "mode"), next.types())
                assertTrue(next.accepted.last().getBoolean("pen_only"))
                capture.disconnect()
                assertEquals(listOf("auth", "resolution", "config"), connection.recover().types())
            } finally { capture.disconnect() }
        }
    }

    @Test fun t387_rejectedLiveSettingsReplayOnlyTheLatestChoice() {
        for (kind in listOf("config", "mode")) {
            val connection = Connection()
            val capture = connection.capture
            try {
                val socket = connection.recover()
                socket.rejectType = kind
                if (kind == "mode") capture.sendMode(true) else capture.sendConfig(2000, 30)
                assertFalse(capture.isControlConnected())
                assertFalse(capture.controlConnected.value)
                capture.sendConfig(4000, 60)
                capture.sendMode(false)
                val next = connection.recover()
                assertEquals(listOf("auth", "resolution", "config", "mode"), next.types())
                assertEquals(4000, next.accepted[2].getInt("bitrate"))
                assertFalse(next.accepted[3].getBoolean("pen_only"))
            } finally { capture.disconnect() }
        }
    }

    private fun touches(action: Int): MotionEvent = MotionEvent.obtain(
        0, 10, action, 2,
        Array(2) { index -> MotionEvent.PointerProperties().apply { id = index; toolType = MotionEvent.TOOL_TYPE_FINGER } },
        Array(2) { MotionEvent.PointerCoords().apply { x = 25f; y = 50f; pressure = 0.5f } },
        0, 0, 1f, 1f, 0, 0, 0, 0
    )

    private fun contact(capture: TouchCapture, action: Int) {
        val event = touches(action)
        try { capture.handleMotionEvent(event, 100, 100) } finally { event.recycle() }
    }

    @Test fun t387_rejectedReleaseRetiresTheWholeGestureWithoutReplayingIt() {
        val connection = Connection()
        val capture = connection.capture
        try {
            val socket = connection.recover()
            contact(capture, MotionEvent.ACTION_DOWN)
            contact(capture, MotionEvent.ACTION_POINTER_DOWN or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT))
            socket.rejectType = "touch"
            contact(capture, MotionEvent.ACTION_CANCEL)
            assertFalse(capture.isControlConnected())
            assertTrue(socket.cancelled)
            val next = connection.recover()
            contact(capture, MotionEvent.ACTION_MOVE)
            assertEquals(listOf("auth", "resolution"), next.types())
            contact(capture, MotionEvent.ACTION_DOWN)
            assertEquals(0, next.accepted.last().getInt("slot"))
        } finally { capture.disconnect() }
    }

    @Test fun t387_rejectedRenderAckRetiresSocketAndOldCallbacksStayStale() {
        val connection = Connection()
        val capture = connection.capture
        try {
            val socket = connection.recover()
            val generation = capture.connectionGeneration
            socket.rejectType = "rendered"
            capture.sendRendered(10, 20)
            assertFalse(capture.isControlConnected())
            assertTrue(socket.cancelled)
            assertTrue(capture.connectionGeneration > generation)
            val next = connection.recover()
            val current = capture.connectionGeneration
            socket.listener.onFailure(socket, java.io.IOException("late failure"), null)
            socket.listener.onClosed(socket, 1000, "late close")
            socket.open()
            assertEquals(current, capture.connectionGeneration)
            capture.sendRendered(11, 30)
            assertEquals(11, next.accepted.last().getInt("seq"))
        } finally { capture.disconnect() }
    }
    @Test fun t387_refusalSchedulesAutomaticRecoveryAndReportsQueuePressure() {
        val connection = Connection()
        val capture = connection.capture
        try {
            val socket = connection.recover()
            connection.created.clear()
            val before = socket.queueSize()
            socket.rejectType = "rendered"
            capture.sendRendered(1, 10)
            val stats = capture.controlStatistics()
            assertEquals(2L, stats.accepted)
            assertEquals(1L, stats.rejected)
            assertEquals(before, stats.peakQueueBytes)
            assertEquals(0L, stats.queueBytes)
            val next = connection.created.poll(5, java.util.concurrent.TimeUnit.SECONDS)
            assertNotNull("T387 failed send must schedule reconnect", next)
            next!!.open()
            next.greet()
            assertTrue(capture.controlConnected.value)
            assertEquals(listOf("auth", "resolution"), next.types())
        } finally { capture.disconnect() }
    }

    @Test fun t387_statisticsUseBoundedScalarsAndClampFutureSamples() {
        val stats = ControlStatistics()
        stats.record(true, 10, 30, 15)
        stats.record(true, 5, 20, -2)
        stats.record(false, 20, 20, null)
        assertEquals(ControlStatisticsSnapshot(2, 1, 7, 30, 2, 15, 15), stats.snapshot(7))
    }

}
