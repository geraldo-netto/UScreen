package com.blent

import android.view.MotionEvent
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createComposeRule
import okhttp3.*
import okio.ByteString
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.LooperMode

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34], qualifiers = "w960dp-h600dp-land", instrumentedPackages = ["androidx.compose.ui.platform"])
@LooperMode(LooperMode.Mode.PAUSED)
class DirectInputTest {
    @get:Rule val compose = createComposeRule()
    private fun status(mode: String = "direct_mouse", ready: Boolean = true) = JSONObject()
        .put("protocol", 1).put("mode", mode).put("touch", true).put("mouse", true).put("negotiated", ready)
    private fun event(action: Int, x: Float = 50f, y: Float = 50f, time: Long = 100, tool: Int = MotionEvent.TOOL_TYPE_FINGER, pointer: Int = 3): MotionEvent {
        val properties = MotionEvent.PointerProperties().apply { id = pointer; toolType = tool }
        val coords = MotionEvent.PointerCoords().apply { this.x = x; this.y = y; pressure = 1f }
        return MotionEvent.obtain(100, time, action, 1, arrayOf(properties), arrayOf(coords), 0, 0, 1f, 1f, 0, 0, 0, 0)
    }
    private fun sample(motion: DirectMotion, action: Int, x: Float = 50f, y: Float = 50f, time: Long = 100, tool: Int = MotionEvent.TOOL_TYPE_FINGER, width: Int = 100): Boolean {
        val event = event(action, x, y, time, tool)
        return try { motion.handle(event, width, 100) } finally { event.recycle() }
    }
    private fun events(messages: List<JSONObject>) = messages.mapNotNull { it.getJSONObject("command").optJSONObject("event") }
    private fun phases(messages: List<JSONObject>) = events(messages).map { it.getString("phase") }

    @Test fun t673_lost_or_reclassified_drag_pointer_releases_button() {
        val messages = mutableListOf<JSONObject>()
        val motion = DirectMotion { value, _ -> messages.add(value) }
        for ((pointer, tool) in listOf(4 to MotionEvent.TOOL_TYPE_FINGER, 3 to 5, 3 to MotionEvent.TOOL_TYPE_STYLUS)) {
            motion.update(null); motion.update(status()); motion.toggleDrag()
            sample(motion, MotionEvent.ACTION_DOWN)
            messages.clear()
            val lost = event(MotionEvent.ACTION_MOVE, tool = tool, pointer = pointer)
            try { motion.handle(lost, 100, 100) } finally { lost.recycle() }
            assertEquals("T673: missing or rejected tracked finger must retire its drag", listOf("cancel"), phases(messages))
            messages.clear(); sample(motion, MotionEvent.ACTION_UP)
            assertTrue(messages.isEmpty())
        }
    }

    @Test fun t673_tap_move_right_click_and_explicit_drag_have_separate_semantics() {
        val messages = mutableListOf<JSONObject>(); val motion = DirectMotion { value, _ -> messages.add(value) }
        assertFalse(sample(motion, MotionEvent.ACTION_DOWN)); motion.rightClick(); motion.toggleDrag(); motion.select("touch")
        for (invalid in listOf<JSONObject?>(null, JSONObject(), status("pen"), status().put("protocol", 2))) { motion.update(invalid); assertNull(motion.state.value) }
        motion.update(status()); motion.rightClick(); assertTrue(messages.isEmpty())
        sample(motion, MotionEvent.ACTION_DOWN); sample(motion, MotionEvent.ACTION_UP, time = 120)
        assertEquals(listOf("move", "move", "down", "up"), phases(messages))
        motion.rightClick(); assertEquals("right", events(messages).last().getString("button"))
        messages.clear(); motion.toggleDrag(); assertTrue(motion.state.value!!.drag)
        sample(motion, MotionEvent.ACTION_DOWN); sample(motion, MotionEvent.ACTION_MOVE, 90f); sample(motion, MotionEvent.ACTION_UP, 90f, time = 150)
        assertEquals(listOf("move", "down", "move", "move", "up"), phases(messages)); assertFalse(motion.state.value!!.drag)
        messages.clear(); sample(motion, MotionEvent.ACTION_DOWN); sample(motion, MotionEvent.ACTION_MOVE, 90f); sample(motion, MotionEvent.ACTION_UP, 90f)
        assertFalse(phases(messages).contains("down"))
        messages.clear(); sample(motion, MotionEvent.ACTION_DOWN); sample(motion, MotionEvent.ACTION_UP, time = 401)
        assertFalse(phases(messages).contains("down"))
        motion.toggleDrag(); sample(motion, MotionEvent.ACTION_DOWN); motion.rightClick(); sample(motion, MotionEvent.ACTION_CANCEL)
        assertEquals("cancel", phases(messages).last())
        motion.select("unknown"); motion.select("touch"); assertFalse(motion.state.value!!.ready)
        assertEquals("select", messages.last().getJSONObject("command").getString("type"))
        motion.select("direct_mouse"); motion.update(status("touch")); assertFalse(sample(motion, MotionEvent.ACTION_DOWN))
    }

    @Test fun t673_bounded_invalid_coordinates_and_tools_cancel_without_clicks_or_stale_replay() {
        val messages = mutableListOf<JSONObject>(); val motion = DirectMotion { value, _ -> messages.add(value) }
        for (x in listOf(-100f, -0.1f, 100.1f, Float.NaN, Float.POSITIVE_INFINITY, Float.NEGATIVE_INFINITY)) {
            motion.update(null); motion.update(status()); motion.toggleDrag(); sample(motion, MotionEvent.ACTION_DOWN)
            messages.clear(); sample(motion, MotionEvent.ACTION_MOVE, x)
            assertEquals(listOf("cancel"), phases(messages))
            messages.clear(); sample(motion, MotionEvent.ACTION_UP, 50f); assertTrue(messages.isEmpty())
        }
        for (tool in -1..8) {
            motion.update(null); motion.update(status()); messages.clear(); sample(motion, MotionEvent.ACTION_DOWN, tool = tool)
            assertEquals(tool == MotionEvent.TOOL_TYPE_FINGER, messages.isNotEmpty())
        }
        motion.update(null); motion.update(status()); motion.toggleDrag(); sample(motion, MotionEvent.ACTION_DOWN); messages.clear()
        motion.toggleDrag(); assertEquals(listOf("cancel"), phases(messages)); assertFalse(motion.state.value!!.drag)
        motion.toggleDrag(); sample(motion, MotionEvent.ACTION_DOWN); messages.clear(); sample(motion, MotionEvent.ACTION_MOVE, width = 0)
        assertEquals(listOf("cancel"), phases(messages))
        motion.update(status()); messages.clear(); sample(motion, MotionEvent.ACTION_MOVE); assertTrue(messages.isEmpty())
        sample(motion, MotionEvent.ACTION_DOWN); motion.forget(); messages.clear(); sample(motion, MotionEvent.ACTION_UP); assertTrue(messages.isEmpty())
        motion.update(status().put("mouse", false).put("touch", false)); motion.select("touch"); motion.toggleDrag(); motion.rightClick(); assertTrue(messages.isEmpty())
    }

    private class Socket(val listener: WebSocketListener) : WebSocket {
        val messages = mutableListOf<JSONObject>()
        var accept = true
        override fun request() = Request.Builder().url(TouchCapture.WS_URL).build()
        override fun queueSize() = 0L
        override fun send(text: String): Boolean { messages.add(JSONObject(text)); return accept }
        override fun send(bytes: ByteString) = false
        override fun close(code: Int, reason: String?) = true
        override fun cancel() {}
        fun open() { listener.onOpen(this, Response.Builder().request(request()).protocol(Protocol.HTTP_1_1).code(101).message("connected").build()) }
        fun greet(status: JSONObject, kind: String = "connected") { listener.onMessage(this, JSONObject().put("status", kind).put("pen", false).put("touch", false).put("direct_input", status).toString()) }
    }

    @Test fun t673_failed_negotiation_send_never_accepts_retired_greeting() {
        lateinit var socket: Socket
        val capture = TouchCapture(WebSocket.Factory { _, listener -> Socket(listener).also { socket = it } })
        capture.token = "b".repeat(64)
        try {
            capture.connect(); socket.open(); socket.accept = false
            socket.greet(status(ready = false))
            assertFalse("T673: failed negotiation cannot authenticate a retired socket", capture.isControlConnected())
            assertFalse(capture.controlConnected.value)
            assertNull(capture.motion.direct.state.value)
        } finally { capture.disconnect() }
    }

    @Test fun t673_negotiation_controls_and_disconnect_preserve_existing_touch_mode() {
        lateinit var socket: Socket
        val capture = TouchCapture(WebSocket.Factory { _, listener -> Socket(listener).also { socket = it } })
        capture.token = "a".repeat(64)
        compose.setContent { BlentTheme { DirectInputControls(capture) } }
        try {
            compose.onNodeWithText("Mouse").assertDoesNotExist()
            capture.selectDirect("touch"); capture.rightClick(); capture.toggleDrag()
            capture.connect(); socket.open(); socket.greet(status(ready = false))
            assertEquals(listOf("auth", "direct_input"), socket.messages.map { it.getString("type") })
            assertEquals("negotiate", socket.messages.last().getJSONObject("command").getString("type"))
            compose.onNodeWithText("Mouse").assertIsNotEnabled()
            socket.greet(status(), "input")
            val down = event(MotionEvent.ACTION_DOWN); val up = event(MotionEvent.ACTION_UP, time = 120)
            capture.handleMotionEvent(down, 100, 100); capture.handleMotionEvent(up, 100, 100); down.recycle(); up.recycle()
            compose.onNodeWithText("Right-click").performClick()
            compose.onNodeWithText("Drag next gesture").performClick(); compose.onNodeWithText("Cancel drag").performClick()
            capture.toggleDrag()
            val held = event(MotionEvent.ACTION_DOWN)
            capture.handleMotionEvent(held, 100, 100); held.recycle()
            socket.greet(status(), "mode")
            val release = event(MotionEvent.ACTION_UP, time = 150)
            capture.handleMotionEvent(release, 100, 100); release.recycle()
            assertEquals("T673: repeated metadata must preserve drag release", "up", socket.messages.last().getJSONObject("command").getJSONObject("event").getString("phase"))
            compose.onNodeWithText("Touch").performClick(); socket.greet(status("touch"), "input")
            val touch = event(MotionEvent.ACTION_DOWN)
            capture.handleMotionEvent(touch, 100, 100); touch.recycle()
            assertEquals("touch", socket.messages.last().getString("type"))
            compose.onNodeWithText("Right-click").assertDoesNotExist()
            compose.onNodeWithText("Mouse").performClick(); socket.greet(status(), "input")
            socket.accept = false; capture.selectDirect("touch")
            assertFalse(capture.isControlConnected()); assertNull(capture.motion.direct.state.value)
            capture.disconnect(); compose.onNodeWithText("Mouse").assertDoesNotExist()
        } finally { capture.disconnect() }
    }
}
