package com.blent

import org.json.JSONObject

// T510: JSON rejects NaN/infinity. Keep the existing endpoint clamp for
// normalized values; unknown readings and non-finite tilt use neutral zero.
private fun normalizedSample(value: Double) = if (value.isNaN()) 0.0 else value.coerceIn(0.0, 1.0)
private fun finiteSample(value: Double) = if (value.isFinite()) value else 0.0

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
        put("x", normalizedSample(x))
        put("y", normalizedSample(y))
        put("pressure", normalizedSample(pressure))
        put("tilt_x", finiteSample(tiltX))
        put("tilt_y", finiteSample(tiltY))
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
        put("x", normalizedSample(x))
        put("y", normalizedSample(y))
        put("pressure", normalizedSample(pressure))
        put("action", action)
        put("slot", slot)
    }
}
