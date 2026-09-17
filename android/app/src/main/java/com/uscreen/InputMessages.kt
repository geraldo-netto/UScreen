package com.uscreen

import org.json.JSONObject

/** Pen actions share one field layout; button/proximity messages intentionally use zero defaults. */
internal data class PenMessage(
    val action: Int,
    val x: Double = 0.0,
    val y: Double = 0.0,
    val pressure: Double = 0.0,
    val tiltX: Double = 0.0,
    val tiltY: Double = 0.0,
    val eraser: Boolean = false,
    val button: Boolean? = null,
) {
    fun toJson() = JSONObject().apply {
        put("type", "pen")
        put("x", x.coerceIn(0.0, 1.0))
        put("y", y.coerceIn(0.0, 1.0))
        put("pressure", pressure.coerceIn(0.0, 1.0))
        put("tilt_x", tiltX)
        put("tilt_y", tiltY)
        put("eraser", eraser)
        put("action", action)
        button?.let { put("button", it) }
    }
}

internal data class TouchMessage(
    val action: Int,
    val slot: Int,
    val x: Double,
    val y: Double,
    val pressure: Double,
) {
    fun toJson() = JSONObject().apply {
        put("type", "touch")
        put("x", x.coerceIn(0.0, 1.0))
        put("y", y.coerceIn(0.0, 1.0))
        put("pressure", pressure.coerceIn(0.0, 1.0))
        put("action", action)
        put("slot", slot)
    }
}
