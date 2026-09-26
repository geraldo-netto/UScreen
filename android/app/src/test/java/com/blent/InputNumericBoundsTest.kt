package com.blent

import android.view.MotionEvent
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34])
class InputNumericBoundsTest {
    private fun finite(message: JSONObject) {
        for (field in listOf("x", "y", "pressure")) {
            val value = message.getDouble(field)
            assertTrue("T510: invalid $field=$value", value.isFinite() && value in 0.0..1.0)
        }
        for (field in listOf("tilt_x", "tilt_y")) {
            if (message.has(field)) assertTrue("T510: invalid $field", message.getDouble(field).isFinite())
        }
    }

    @Test fun t510_nonFiniteAndExtremeSamplesAlwaysHaveValidBoundedJson() {
        val random = java.util.Random(510)
        val values = listOf(Double.NaN, Double.POSITIVE_INFINITY, Double.NEGATIVE_INFINITY,
            -Double.MAX_VALUE, Double.MAX_VALUE, -1.0, 0.0, 1.0, 2.0) +
            List(2048) { java.lang.Double.longBitsToDouble(random.nextLong()) }
        for (value in values) {
            val pen = PenMessage(1, value, value, value, value, -value, true, false).toJson()
            val touch = TouchMessage(1, 3, value, value, value).toJson()
            finite(pen); finite(touch)
            assertEquals(1, pen.getInt("action"))
            assertTrue(pen.getBoolean("eraser"))
            assertFalse(pen.getBoolean("button"))
            assertEquals(1, touch.getInt("action"))
            assertEquals(3, touch.getInt("slot"))
        }
        val missing = PenMessage(3, Double.NaN, Double.NaN, Double.NaN,
            Double.POSITIVE_INFINITY, Double.NEGATIVE_INFINITY).toJson()
        for (field in listOf("x", "y", "pressure", "tilt_x", "tilt_y")) assertEquals(0.0, missing.getDouble(field), 0.0)
        assertEquals(1.0, TouchMessage(0, 0, Double.POSITIVE_INFINITY, 0.0, 0.0).toJson().getDouble("x"), 0.0)
    }

    private fun event(tool: Int, action: Int): MotionEvent = MotionEvent.obtain(0, 10, action, 1,
        arrayOf(MotionEvent.PointerProperties().apply { id = 0; toolType = tool }),
        arrayOf(MotionEvent.PointerCoords().apply {
            x = Float.NaN; y = Float.POSITIVE_INFINITY; pressure = Float.NaN
            setAxisValue(MotionEvent.AXIS_TILT, Float.NEGATIVE_INFINITY)
            setAxisValue(MotionEvent.AXIS_ORIENTATION, Float.NaN)
        }), 0, 0, 1f, 1f, 0, 0, 0, 0)

    @Test fun t510_malformedPlatformSamplesCannotThrowOrLoseTheContactRelease() {
        for (tool in listOf(MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS, MotionEvent.TOOL_TYPE_ERASER)) {
            val messages = mutableListOf<JSONObject>()
            val translator = MotionTranslator { message, _ -> messages.add(message) }
            for (action in listOf(MotionEvent.ACTION_DOWN, MotionEvent.ACTION_UP)) {
                val sample = event(tool, action)
                try { assertTrue(translator.handleMotionEvent(sample, Int.MIN_VALUE, 0)) }
                finally { sample.recycle() }
            }
            assertEquals(listOf(0, 1), messages.map { it.getInt("action") })
            messages.forEach(::finite)
        }
    }
}
