package com.blent

import android.view.MotionEvent
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.util.ReflectionHelpers
import org.robolectric.util.ReflectionHelpers.ClassParameter

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34])
class InputAccessorBoundsTest {
    @Test fun t497_missingPointerAndHistoryAxesFallBackWithoutInvalidValues() {
        val motion = MotionTranslator { _, _ -> error("T497 accessor must not send") }
        val event = MotionEvent.obtain(0, 1, MotionEvent.ACTION_MOVE, 5f, 6f, 0)
        val random = java.util.Random(497)
        val bounds = listOf(Int.MIN_VALUE, -1, 1, Int.MAX_VALUE) + List(128) { random.nextInt(Int.MAX_VALUE - 1) + 1 }
        try {
            for (index in bounds) {
                val pen: Boolean = ReflectionHelpers.callInstanceMethod(motion, "isEraser",
                    ClassParameter.from(MotionEvent::class.java, event), ClassParameter.from(Int::class.javaPrimitiveType, index))
                assertFalse(pen)
                val axis: Double = ReflectionHelpers.callInstanceMethod(motion, "getAxis",
                    ClassParameter.from(MotionEvent::class.java, event), ClassParameter.from(Int::class.javaPrimitiveType, MotionEvent.AXIS_TILT),
                    ClassParameter.from(Int::class.javaPrimitiveType, index))
                assertEquals(0.0, axis, 0.0)
                val past: Double = ReflectionHelpers.callInstanceMethod(motion, "getHistoricalAxis",
                    ClassParameter.from(MotionEvent::class.java, event), ClassParameter.from(Int::class.javaPrimitiveType, MotionEvent.AXIS_TILT),
                    ClassParameter.from(Int::class.javaPrimitiveType, 0), ClassParameter.from(Int::class.javaPrimitiveType, index))
                assertEquals(0.0, past, 0.0)
            }
        } finally { event.recycle() }
    }

    @Test fun t497_inputFacadeKeepsSettingsCallbacksAndOfflineMode() {
        val capture = TouchCapture(okhttp3.WebSocket.Factory { _, _ -> error("T497 offline facade") })
        assertFalse(capture.isPenOnly)
        var fps = 0
        var format: DecoderFormat? = null
        capture.onFpsKnown = { fps = it }
        capture.onStreamFormat = { format = it }
        capture.onFpsKnown!!(60)
        val expected = DecoderFormat(VideoReceiver.MIME_TYPE, 640, 480, 60)
        capture.onStreamFormat!!(expected)
        assertEquals(60, fps)
        assertSame(expected, format)
        capture.onStreamFormat = null
        capture.onFpsKnown = null
        assertNull(capture.onStreamFormat)
        assertNull(capture.onFpsKnown)
    }
}
