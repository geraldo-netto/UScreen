package com.uscreen

import android.view.MotionEvent
import android.view.SurfaceView
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34])
class SessionSurfaceContractTest {
    @Test fun t497_firstRenderedFrameThanksOnceAndSurfaceRetirementStaysOffline() {
        val app = RuntimeEnvironment.getApplication()
        val prefs = Prefs(app).apply { checkUpdates = false }
        val receiver = VideoReceiver { error("T497 must not open a video socket") }
        val control = TouchCapture(okhttp3.WebSocket.Factory { _, _ -> error("T497 must not connect") })
        val session = SessionCoordinator(prefs, { it() }, receiver, control)
        try {
            assertFalse(session.showThanks)
            receiver.onFrameRendered!!(1, 123)
            assertTrue(session.showThanks)
            assertTrue(Prefs(app).thankedOnce)
            session.dismissThanks()
            receiver.onFrameRendered!!(2, 123)
            assertFalse(session.showThanks)
            val view = SurfaceView(app)
            session.surfaceReady(view)
            val event = MotionEvent.obtain(0, 0, MotionEvent.ACTION_HOVER_MOVE, 10f, 10f, 0)
            try {
                assertFalse(session.hover(event, 0, 100))
                assertFalse(session.hover(event, 100, 0))
                assertFalse(session.hover(event, 100, 100))
                assertFalse(view.dispatchGenericMotionEvent(event))
            } finally { event.recycle() }
            val touch = MotionEvent.obtain(0, 0, MotionEvent.ACTION_DOWN, 10f, 10f, 0)
            try {
                assertTrue("T497 surface must consume touches even while disconnected", view.dispatchTouchEvent(touch))
                assertFalse(control.isControlConnected())
            } finally { touch.recycle() }
            session.surfaceDestroyed()
            assertNull(receiver.decoder.mediaCodec)
        } finally { session.stop() }
    }
}
