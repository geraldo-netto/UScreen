package com.uscreen

import android.view.MotionEvent
import android.view.InputDevice
import android.view.SurfaceView
import okhttp3.*
import okio.ByteString
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34])
class PenLifecycleTest {
    private class Socket(val listener: WebSocketListener) : WebSocket {
        val messages = mutableListOf<JSONObject>()
        override fun request() = Request.Builder().url(TouchCapture.WS_URL).build()
        override fun queueSize() = 0L
        override fun send(text: String): Boolean { messages.add(JSONObject(text)); return true }
        override fun send(bytes: ByteString) = false
        override fun close(code: Int, reason: String?) = true
        override fun cancel() {}
        fun open() { listener.onOpen(this, Response.Builder().request(request()).protocol(Protocol.HTTP_1_1)
            .code(101).message("Switching Protocols").build()) }
    }

    private fun event(tool: Int, step: JSONObject): MotionEvent = MotionEvent.obtain(
        0, 10, step.getInt("android_action"), 1,
        arrayOf(MotionEvent.PointerProperties().apply { id = 0; toolType = tool }),
        arrayOf(MotionEvent.PointerCoords().apply { x = 25f; y = 50f; pressure = 0.5f }),
        0, step.getInt("button_state"), 1f, 1f, 0, 0, InputDevice.SOURCE_STYLUS, 0
    )

    private fun compare(name: String, expected: JSONObject, actual: JSONObject) {
        assertEquals(name, expected.keys().asSequence().toSet(), actual.keys().asSequence().toSet())
        for (key in expected.keys()) {
            val value = expected.get(key)
            if (value is Number) assertEquals("$name $key", value.toDouble(), actual.getDouble(key), 0.00001)
            else assertEquals("$name $key", value, actual.get(key))
        }
    }

    private fun forward(capture: TouchCapture, surface: SurfaceView?, event: MotionEvent) {
        val hover = event.actionMasked in listOf(MotionEvent.ACTION_HOVER_ENTER,
            MotionEvent.ACTION_HOVER_MOVE, MotionEvent.ACTION_HOVER_EXIT)
        if (surface != null && hover) surface.dispatchGenericMotionEvent(event)
        else if (!capture.handleHoverEvent(event, 100, 100)) capture.handleMotionEvent(event, 100, 100)
    }

    private fun replay(case: JSONObject, useSurface: Boolean) {
        lateinit var socket: Socket
        val capture = TouchCapture(WebSocket.Factory { _, listener -> Socket(listener).also { socket = it } })
        val surface = if (useSurface) SurfaceView(RuntimeEnvironment.getApplication()).also {
            it.layout(0, 0, 100, 100); capture.setSurfaceView(it)
        } else null
        capture.connect()
        socket.open()
        try {
            val steps = case.getJSONArray("events")
            for (index in 0 until steps.length()) {
                val step = steps.getJSONObject(index)
                if (step.getString("name") == "reconnect-enter") {
                    capture.disconnect(); capture.connect(); socket.open()
                }
                socket.messages.clear()
                val event = event(case.getInt("tool"), step)
                try { forward(capture, surface, event) } finally { event.recycle() }
                assertEquals(step.getString("name"), 1, socket.messages.size)
                compare(step.getString("name"), step.getJSONObject("wire"), socket.messages.single())
            }
        } finally { capture.disconnect() }
    }

    @Test fun t318_buttonStateSurvivesContactAndEndsOnExitOrCancel() {
        val cases = JSONArray(javaClass.getResource("/pen-lifecycle.json")!!.readText())
        for (index in 0 until cases.length()) {
            for (surface in listOf(false, true)) replay(cases.getJSONObject(index), surface)
        }
    }
}
