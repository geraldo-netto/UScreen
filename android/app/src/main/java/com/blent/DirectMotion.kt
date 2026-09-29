package com.blent

import android.view.MotionEvent
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import org.json.JSONObject
import kotlin.math.abs

internal data class DirectControlsState(
    val mode: String, val touch: Boolean, val mouse: Boolean, val ready: Boolean,
    val drag: Boolean = false,
)

/** Finger mouse gestures are independent of stylus translation and native APIs. */
internal class DirectMotion(private val send: (JSONObject, Long?) -> Unit) {
    private val current = MutableStateFlow<DirectControlsState?>(null)
    val state = current.asStateFlow()
    private var pointer: Int? = null
    private var point: Pair<Double, Double>? = null
    private var origin = 0.0 to 0.0
    private var started = 0L
    private var moved = false
    private var held = false

    fun update(status: JSONObject?) {
        val next = parse(status)
        if (current.value?.copy(drag = false) == next) return
        forget()
        current.value = next
    }

    private fun parse(status: JSONObject?): DirectControlsState? {
        if (status?.optInt("protocol") != 1) return null
        val mode = status.optString("mode")
        if (mode !in listOf("touch", "direct_mouse")) return null
        return DirectControlsState(mode, status.optBoolean("touch"), status.optBoolean("mouse"), status.optBoolean("negotiated"))
    }

    fun forget() { pointer = null; point = null; held = false; moved = false }

    fun select(mode: String) {
        val value = current.value ?: return
        val allowed = when (mode) { "touch" -> value.touch; "direct_mouse" -> value.mouse; else -> false }
        if (!value.ready || !allowed) return
        cancel()
        current.value = value.copy(ready = false, drag = false)
        send(command(JSONObject().put("type", "select").put("mode", mode)), null)
    }

    fun rightClick() {
        if (!mouseReady() || held) return
        emit("down", "right")
        emit("up", "right")
    }

    fun toggleDrag() {
        val value = current.value ?: return
        if (!mouseReady()) return
        cancel()
        current.value = value.copy(drag = !value.drag)
    }

    private fun mouseReady(): Boolean = current.value?.let { it.ready && it.mode == "direct_mouse" && it.mouse } == true

    fun handle(event: MotionEvent, width: Int, height: Int): Boolean {
        if (!mouseReady()) return false
        if (event.actionMasked == MotionEvent.ACTION_CANCEL) { cancel(); return true }
        if (width <= 0 || height <= 0) { cancel(); return true }
        val index = if (event.actionMasked == MotionEvent.ACTION_MOVE) event.findPointerIndex(pointer ?: -1) else event.actionIndex
        if (index < 0) { cancel(); return true }
        if (event.getToolType(index) != MotionEvent.TOOL_TYPE_FINGER) {
            if (pointer == event.getPointerId(index)) cancel()
            return true
        }
        sample(event, index, width, height)
        return true
    }

    private fun sample(event: MotionEvent, index: Int, width: Int, height: Int) {
        val next = event.getX(index).toDouble() / width to event.getY(index).toDouble() / height
        if (next.first !in 0.0..1.0 || next.second !in 0.0..1.0) { cancel(); return }
        if (event.actionMasked == MotionEvent.ACTION_DOWN) { begin(event, index, next); return }
        if (pointer != event.getPointerId(index)) return
        point = next
        moved = moved || abs(next.first - origin.first) * width > 12 || abs(next.second - origin.second) * height > 12
        emit("move", time = event.eventTime)
        if (event.actionMasked == MotionEvent.ACTION_UP || event.actionMasked == MotionEvent.ACTION_POINTER_UP) finish(event.eventTime)
    }

    private fun begin(event: MotionEvent, index: Int, next: Pair<Double, Double>) {
        cancel()
        pointer = event.getPointerId(index)
        point = next
        origin = next
        started = event.eventTime
        moved = false
        emit("move", time = event.eventTime)
        held = current.value?.drag == true
        if (held) emit("down", "left", event.eventTime)
    }

    private fun finish(time: Long) {
        if (held) emit("up", "left", time)
        else if (!moved && time - started in 0..300) {
            emit("down", "left", time)
            emit("up", "left", time)
        }
        pointer = null
        held = false
        current.value = current.value?.copy(drag = false)
    }

    private fun cancel() {
        if (held) emit("cancel", "left")
        held = false
        pointer = null
    }

    private fun emit(phase: String, button: String? = null, time: Long? = null) {
        val at = point ?: return
        val event = JSONObject().put("type", "mouse").put("x", at.first).put("y", at.second)
            .put("phase", phase).put("button", button ?: JSONObject.NULL)
        send(command(JSONObject().put("type", "event").put("event", event)), time)
    }

    private fun command(value: JSONObject) = JSONObject().put("type", "direct_input").put("command", value)
}
