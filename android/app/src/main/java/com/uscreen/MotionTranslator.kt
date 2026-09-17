package com.uscreen

import android.view.MotionEvent
import kotlin.math.atan2
import kotlin.math.cos
import kotlin.math.sin
import org.json.JSONObject

/** Translates Android samples into ordered wire events; owns pointer-to-slot assignments. */
internal class MotionTranslator(private val send: (JSONObject, Long?) -> Unit) : ControlInputState {
    private companion object { const val TOOL_TYPE_PALM = 5 }
    private val touchSlots = mutableMapOf<Int, Int>()
    @Volatile private var touchEnabled = true
    @Volatile private var penEnabled = true

    override fun reset() {
        forgetTouches()
        touchEnabled = true
        penEnabled = true
    }

    override fun forgetTouches() { touchSlots.clear() }
    override fun setTouchEnabled(enabled: Boolean) {
        touchEnabled = enabled
        if (!enabled) forgetTouches()
    }
    override fun setPenEnabled(enabled: Boolean) { penEnabled = enabled }

    // Surface listeners consume all enabled hover events; Activity generic motion
    // only consumes the supported stylus actions (including side buttons).
    fun handleSurfaceHover(event: MotionEvent, width: Int, height: Int): Boolean {
        if (!penEnabled) return false
        val vw = width.coerceAtLeast(1).toFloat()
        val vh = height.coerceAtLeast(1).toFloat()
        when (event.actionMasked) {
            MotionEvent.ACTION_HOVER_ENTER,
            MotionEvent.ACTION_HOVER_MOVE -> {
                if (isPenLike(event, 0)) sendPenEvent(event, 0, 3, vw, vh)
            }
            MotionEvent.ACTION_HOVER_EXIT -> {
                if (isPenLike(event, 0)) sendPenProximityExit(event.eventTime)
            }
        }
        return true
    }

    /**
     * Forward stylus hover so the host's cursor follows the pen before it
     * touches down. Returns true only for pen hover, so nothing else the
     * activity might want to do with generic motion events is disturbed.
     */
    fun handleHoverEvent(event: MotionEvent, width: Int, height: Int): Boolean {
        if (!penEnabled || !isPenLike(event, 0)) return false
        val vw = width.coerceAtLeast(1).toFloat()
        val vh = height.coerceAtLeast(1).toFloat()
        return when (event.actionMasked) {
            MotionEvent.ACTION_HOVER_ENTER,
            MotionEvent.ACTION_HOVER_MOVE -> {
                sendPenEvent(event, 0, 3, vw, vh)
                true
            }
            MotionEvent.ACTION_HOVER_EXIT -> {
                sendPenProximityExit(event.eventTime)
                true
            }
            // S-Pen side button. Android delivers BUTTON_PRESS/RELEASE as
            // generic motion, never through the touch listener, so this is
            // the only place they can be caught. Forwarded as the stylus
            // button (right-click in GIMP).
            MotionEvent.ACTION_BUTTON_PRESS -> {
                sendPenButton(true, event.eventTime)
                true
            }
            MotionEvent.ACTION_BUTTON_RELEASE -> {
                sendPenButton(false, event.eventTime)
                true
            }
            else -> false
        }
    }

    fun handleMotionEvent(event: MotionEvent, width: Int, height: Int): Boolean {
        if (!touchEnabled && !penEnabled) return false
        val vw = width.coerceAtLeast(1).toFloat()
        val vh = height.coerceAtLeast(1).toFloat()
        releasePalmContacts(event)
        if (event.actionMasked == MotionEvent.ACTION_DOWN) releaseTouches()
        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN,
            MotionEvent.ACTION_POINTER_DOWN -> sendContact(event, event.actionIndex, 0, vw, vh)
            MotionEvent.ACTION_MOVE -> sendMotionSamples(event, vw, vh)
            MotionEvent.ACTION_UP,
            MotionEvent.ACTION_POINTER_UP -> sendContact(event, event.actionIndex, 1, vw, vh)
            MotionEvent.ACTION_CANCEL -> cancelContacts(event)
        }
        return true
    }

    private fun releasePalmContacts(event: MotionEvent) {
        for (index in 0 until event.pointerCount) {
            if (!isPalm(event, index)) continue
            val slot = touchSlots.remove(event.getPointerId(index)) ?: continue
            sendTouch(0f, 0f, 0.0, 1, slot)
        }
    }

    private fun sendContact(event: MotionEvent, index: Int, action: Int, vw: Float, vh: Float) {
        // Reject a platform palm marker if one reaches the app.
        if (isPalm(event, index)) return
        if (isPenLike(event, index)) {
            sendPenEvent(event, index, action, vw, vh)
        } else {
            sendFinger(event, index, action, vw, vh)
        }
    }

    private fun canForwardPointer(event: MotionEvent, index: Int): Boolean {
        if (isPalm(event, index)) return false
        return if (isPenLike(event, index)) penEnabled else touchEnabled
    }

    private fun sendMotionSamples(event: MotionEvent, vw: Float, vh: Float) {
        for (i in 0 until event.pointerCount) {
            if (!canForwardPointer(event, i)) continue
            if (isPenLike(event, i)) sendPenHistory(event, i, vw, vh)
            sendContact(event, i, 2, vw, vh)
        }
    }

    private fun sendPenHistory(event: MotionEvent, index: Int, vw: Float, vh: Float) {
        // Android batches samples between frames. Forward history too so fast
        // pen strokes retain their shape in drawing applications.
        for (h in 0 until event.historySize) {
            val hx = event.getHistoricalX(index, h) / vw
            val hy = event.getHistoricalY(index, h) / vh
            val hp = event.getHistoricalPressure(index, h).toDouble()
            val (htx, hty) = decomposeTilt(
                getHistoricalAxis(event, MotionEvent.AXIS_TILT, index, h),
                getHistoricalAxis(event, MotionEvent.AXIS_ORIENTATION, index, h))
            emitPen(hx.toDouble(), hy.toDouble(), hp, htx, hty,
                isEraser(event, index), 2, event.getHistoricalEventTime(h), primaryButtonDown(event))
        }
    }

    private fun cancelContacts(event: MotionEvent) {
        // Release each device independently: a touch release cannot lift a pen.
        for (i in 0 until event.pointerCount) {
            if (!canForwardPointer(event, i)) continue
            if (isPenLike(event, i)) sendPenProximityExit(event.eventTime)
        }
        releaseTouches()
    }

    // Android pointer IDs can be sparse and exceed the host's 10 slots.
    // Allocate only on DOWN and retain the assignment until UP/CANCEL.
    private fun sendFinger(event: MotionEvent, index: Int, action: Int, vw: Float, vh: Float) {
        if (!touchEnabled) return
        val id = event.getPointerId(index)
        val slot = touchSlots[id] ?: if (action == 0) {
            val free = (0..9).firstOrNull { it !in touchSlots.values } ?: return
            touchSlots[id] = free
            free
        } else return
        sendTouch(event.getX(index) / vw, event.getY(index) / vh,
            if (action == 1) 0.0 else event.getPressure(index).toDouble(), action, slot, event.eventTime)
        if (action == 1) touchSlots.remove(id)
    }

    private fun releaseTouches() {
        // A refused send retires the connection and clears slots synchronously.
        for (slot in touchSlots.values.toList()) sendTouch(0f, 0f, 0.0, 1, slot)
        touchSlots.clear()
    }

    /** Stylus or its eraser end — both drive the pen/tablet device. */
    private fun isPenLike(event: MotionEvent, index: Int): Boolean {
        return try {
            val t = event.getToolType(index)
            t == MotionEvent.TOOL_TYPE_STYLUS || t == MotionEvent.TOOL_TYPE_ERASER
        } catch (_: Exception) {
            false
        }
    }

    private fun isEraser(event: MotionEvent, index: Int): Boolean {
        return try {
            event.getToolType(index) == MotionEvent.TOOL_TYPE_ERASER
        } catch (_: Exception) {
            false
        }
    }

    private fun getAxis(event: MotionEvent, axis: Int, index: Int): Double {
        return try {
            event.getAxisValue(axis, index).toDouble()
        } catch (_: Exception) {
            0.0
        }
    }

    private fun getHistoricalAxis(event: MotionEvent, axis: Int, index: Int, hist: Int): Double {
        return try {
            event.getHistoricalAxisValue(axis, index, hist).toDouble()
        } catch (_: Exception) {
            0.0
        }
    }

    /**
     * Decompose Android's stylus tilt into X/Y tilt angles, **in degrees**.
     *
     * Android exposes AXIS_TILT as the angle from the screen normal (0 =
     * perpendicular, π/2 = flat) and AXIS_ORIENTATION as the azimuth of the
     * tilt around that normal (0..2π). A Wacom-style ABS_TILT_X/Y device wants
     * the signed X and Y tilt *angles*, which are the arctangents of the tilt
     * vector's components projected onto the surface — not the components
     * themselves.
     *
     * The previous version returned the raw projections `sin(tilt)·cos(orient)`
     * (a dimensionless value in [-1,1]) and the host multiplied them by
     * 180/π as if they were radians. A pen laid flat at 90° came out as 57°,
     * and everything in between was wrong non-linearly.
     */
    private fun decomposeTilt(tiltRad: Double, orientationRad: Double): Pair<Double, Double> {
        val sinTilt = sin(tiltRad)
        val cosTilt = cos(tiltRad)
        // Android's AXIS_ORIENTATION is measured clockwise from the top of the
        // screen: 0 points up, +pi/2 points right. So the direction the pen
        // leans, in screen coordinates with y downwards, is
        // (sin(orientation), -cos(orientation)).
        //
        // libinput wants tilt_x positive towards the right edge and tilt_y
        // positive towards the user, which is the bottom edge — hence the
        // minus on the y component. These two were the other way round until
        // 1.2.2, so a pen leaning right reported as leaning down; reported by
        // a Lenovo Tab Pen Plus user in issue #11.
        val tx = atan2(sinTilt * sin(orientationRad), cosTilt)
        val ty = atan2(-sinTilt * cos(orientationRad), cosTilt)
        return Math.toDegrees(tx) to Math.toDegrees(ty)
    }

    private fun sendPenEvent(event: MotionEvent, index: Int, action: Int,
                              vw: Float, vh: Float) {
        if (!penEnabled) return
        val x = event.getX(index) / vw
        val y = event.getY(index) / vh
        val pressure = event.getPressure(index).toDouble()
        val (tiltX, tiltY) = decomposeTilt(
            getAxis(event, MotionEvent.AXIS_TILT, index),
            getAxis(event, MotionEvent.AXIS_ORIENTATION, index))
        emitPen(x.toDouble(), y.toDouble(), pressure, tiltX, tiltY,
            isEraser(event, index), action, event.eventTime, primaryButtonDown(event))
    }

    private fun emitPen(x: Double, y: Double, pressure: Double,
                        tiltX: Double, tiltY: Double, eraser: Boolean, action: Int, sampleTimeMs: Long, button: Boolean) {
        if (!penEnabled) return
        send(PenMessage(action, x, y, pressure, tiltX, tiltY, eraser, button).toJson(), sampleTimeMs)
    }

    private fun primaryButtonDown(event: MotionEvent): Boolean =
        event.buttonState and MotionEvent.BUTTON_STYLUS_PRIMARY != 0

    private fun sendPenButton(down: Boolean, sampleTimeMs: Long) {
        if (penEnabled) send(PenMessage(if (down) 5 else 6).toJson(), sampleTimeMs)
    }

    private fun sendPenProximityExit(sampleTimeMs: Long) {
        if (penEnabled) send(PenMessage(4).toJson(), sampleTimeMs)
    }

    private fun sendTouch(x: Float, y: Float, pressure: Double, action: Int, slot: Int, sampleTimeMs: Long? = null) {
        if (touchEnabled) send(TouchMessage(action, slot, x.toDouble(), y.toDouble(), pressure).toJson(), sampleTimeMs)
    }

    /**
     * AOSP defines the hidden MotionEvent.TOOL_TYPE_PALM as 5. Keep the
     * numeric value because it is outside the public SDK. Android normally
     * filters palms before delivery; this also rejects any that reach us.
     * T303: frameworks/base/core/java/android/view/MotionEvent.java.
     */
    @android.annotation.SuppressLint("WrongConstant")
    private fun isPalm(event: MotionEvent, index: Int): Boolean =
        event.getToolType(index) == TOOL_TYPE_PALM

}
