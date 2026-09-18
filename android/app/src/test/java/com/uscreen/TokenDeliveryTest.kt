package com.uscreen

import android.os.Looper
import android.content.ComponentName
import android.content.Intent
import okhttp3.Request
import okhttp3.WebSocket
import okio.ByteString
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.annotation.LooperMode

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34])
@LooperMode(LooperMode.Mode.PAUSED)
class TokenDeliveryTest {
    @Test fun t420_receiver_requires_shell_permission_and_never_launches_ui_or_service() {
        val app = RuntimeEnvironment.getApplication()
        val component = ComponentName(app.packageName, TokenReceiver::class.java.name)
        val info = app.packageManager.getReceiverInfo(component, 0)
        assertTrue(info.exported)
        assertEquals("android.permission.DUMP", info.permission)
        val token = "c".repeat(64)
        TokenReceiver().onReceive(app, Intent().setComponent(component).putExtra("token", token))
        assertEquals(token, Prefs(app).hostToken)
        assertNull("T420: recovery stole foreground", shadowOf(app).nextStartedActivity)
        assertNull("T420: background delivery started a service", shadowOf(app).nextStartedService)
        for (invalid in listOf("bad", "z".repeat(64))) {
            TokenReceiver().onReceive(app, Intent().putExtra("token", invalid))
            assertEquals(token, Prefs(app).hostToken)
        }
        TokenReceiver().onReceive(app, Intent())
        assertEquals(token, Prefs(app).hostToken)
    }

    private class Socket : WebSocket {
        override fun request() = Request.Builder().url(TouchCapture.WS_URL).build()
        override fun queueSize() = 0L
        override fun send(text: String) = true
        override fun send(bytes: ByteString) = true
        override fun close(code: Int, reason: String?) = true
        override fun cancel() {}
    }

    @Test fun t420_live_token_delivery_restarts_waiting_control_without_activity_launch() {
        val prefs = Prefs(RuntimeEnvironment.getApplication()).apply { hostToken = "a".repeat(64) }
        val capture = TouchCapture(WebSocket.Factory { _, _ -> Socket() })
        val session = SessionCoordinator(prefs, { it() }, null, capture)
        session.start()
        try {
            val old = capture.connectionGeneration
            prefs.hostToken = "b".repeat(64)
            shadowOf(Looper.getMainLooper()).idle()
            assertEquals("T420: foreground retries must consume delivered credentials", prefs.hostToken, capture.token)
            assertTrue(capture.connectionGeneration > old)
        } finally { session.stop() }
    }

    @Test fun t420_background_delivery_stays_stopped_and_is_consumed_on_resume() {
        val prefs = Prefs(RuntimeEnvironment.getApplication()).apply { hostToken = "a".repeat(64) }
        val capture = TouchCapture(WebSocket.Factory { _, _ -> Socket() })
        val session = SessionCoordinator(prefs, { it() }, null, capture)
        session.start()
        session.stop()
        val stopped = capture.connectionGeneration
        prefs.hostToken = "b".repeat(64)
        shadowOf(Looper.getMainLooper()).idle()
        assertEquals("T420: background delivery must not connect", stopped, capture.connectionGeneration)
        session.start()
        try { assertEquals("T420: resume must use the latest delivered token", prefs.hostToken, capture.token) }
        finally { session.stop() }
    }
}
