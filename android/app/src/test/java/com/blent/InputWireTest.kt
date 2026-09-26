package com.blent

import android.view.MotionEvent
import okhttp3.Request
import okhttp3.WebSocket
import okio.ByteString
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34])
class InputWireTest {
    private class Socket : WebSocket {
        val messages = mutableListOf<JSONObject>()
        override fun request() = Request.Builder().url(TouchCapture.WS_URL).build()
        override fun queueSize() = 0L
        override fun send(text: String): Boolean { messages.add(JSONObject(text)); return true }
        override fun send(bytes: ByteString) = false
        override fun close(code: Int, reason: String?) = true
        override fun cancel() {}
    }

    private fun capture(socket: Socket): TouchCapture {
        val capture = TouchCapture()
        ControlSession::class.java.getDeclaredField("webSocket").apply { isAccessible = true }.set(capture.control, socket)
        ControlSession::class.java.getDeclaredField("isConnected").apply { isAccessible = true }.set(capture.control, true)
        return capture
    }

    private fun event(tool: Int, action: Int, tilt: Float = 0f): MotionEvent = MotionEvent.obtain(
        0, 10, action, 1,
        arrayOf(MotionEvent.PointerProperties().apply { id = 0; toolType = tool }),
        arrayOf(MotionEvent.PointerCoords().apply {
            x = 25f; y = 50f; pressure = 0.5f
            setAxisValue(MotionEvent.AXIS_TILT, tilt)
        }), 0, 0, 1f, 1f, 0, 0, 0, 0
    )

    private fun compare(expected: JSONObject, actual: JSONObject) {
        assertEquals(expected.keys().asSequence().toSet(), actual.keys().asSequence().toSet())
        for (key in expected.keys()) {
            val value = expected.get(key)
            if (value is Number) assertEquals(key, value.toDouble(), actual.getDouble(key), 0.00001)
            else assertEquals(key, value, actual.get(key))
        }
    }

    @Test fun t375_motionMessagesAgreeWithTheHostWireFixture() {
        val cases = JSONArray(javaClass.getResource("/input-motion.json")!!.readText())
        val socket = Socket()
        val capture = capture(socket)
        try {
            for (index in 0 until cases.length()) {
                val case = cases.getJSONObject(index)
                socket.messages.clear()
                val event = event(case.getInt("tool"), case.getInt("android_action"))
                try {
                    if (!capture.handleHoverEvent(event, 100, 100)) capture.handleMotionEvent(event, 100, 100)
                } finally { event.recycle() }
                assertEquals(case.getString("name"), 1, socket.messages.size)
                compare(case.getJSONObject("wire"), socket.messages.single())
            }
        } finally { capture.disconnect() }
    }

    @Test fun t375_historicalEraserSamplesKeepOrderTiltAndPressure() {
        val socket = Socket()
        val capture = capture(socket)
        val event = event(MotionEvent.TOOL_TYPE_ERASER, MotionEvent.ACTION_MOVE, (Math.PI / 4).toFloat())
        val next = MotionEvent.PointerCoords().apply {
            x = 75f; y = 25f; pressure = 0.75f
            setAxisValue(MotionEvent.AXIS_TILT, (Math.PI / 6).toFloat())
            setAxisValue(MotionEvent.AXIS_ORIENTATION, (Math.PI / 2).toFloat())
        }
        event.addBatch(20, arrayOf(next), 0)
        try {
            capture.handleMotionEvent(event, 100, 100)
            assertEquals(2, socket.messages.size)
            val first = socket.messages[0]
            val last = socket.messages[1]
            assertEquals(0.25, first.getDouble("x"), 0.00001)
            assertEquals(0.5, first.getDouble("pressure"), 0.00001)
            assertEquals(-45.0, first.getDouble("tilt_y"), 0.00001)
            assertEquals(0.75, last.getDouble("x"), 0.00001)
            assertEquals(0.75, last.getDouble("pressure"), 0.00001)
            assertEquals(30.0, last.getDouble("tilt_x"), 0.00001)
            for (message in socket.messages) {
                assertTrue(message.getBoolean("eraser"))
                assertEquals(2, message.getInt("action"))
            }
        } finally { event.recycle(); capture.disconnect() }
    }
}
