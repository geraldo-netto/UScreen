package com.uscreen

import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.pm.ApplicationInfo
import android.view.MotionEvent
import android.view.OrientationEventListener
import okhttp3.*
import okio.ByteString
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34])
class RegressionTest {
    private val app get() = RuntimeEnvironment.getApplication()
    private fun get(target: Any, name: String): Any? = target.javaClass.getDeclaredField(name).apply { isAccessible = true }.get(target)
    private fun set(target: Any, name: String, value: Any?) = target.javaClass.getDeclaredField(name).apply { isAccessible = true }.set(target, value)

    @Test fun t036_sessionTokenIsExcludedFromBackup() {
        assertEquals(0, app.applicationInfo.flags and ApplicationInfo.FLAG_ALLOW_BACKUP)
    }

    @Test fun t037_cleartextPolicyOnlyPermitsLoopback() {
        val id = app.resources.getIdentifier("network_security_config", "xml", app.packageName)
        assertTrue("Scoped network security policy must exist", id != 0)
        val parser = app.resources.getXml(id)
        var baseDisallows = false
        val domains = mutableListOf<String>()
        while (parser.next() != org.xmlpull.v1.XmlPullParser.END_DOCUMENT) {
            if (parser.eventType != org.xmlpull.v1.XmlPullParser.START_TAG) continue
            when (parser.name) {
                "base-config" -> baseDisallows = parser.getAttributeValue(null, "cleartextTrafficPermitted") == "false"
                "domain-config" -> assertEquals("true", parser.getAttributeValue(null, "cleartextTrafficPermitted"))
                "domain" -> domains.add(parser.nextText())
            }
        }
        assertTrue(baseDisallows)
        assertEquals(listOf("127.0.0.1"), domains)
    }

    @Test fun t039_launcherIntentCannotOverwriteTrustedToken() {
        val prefs = Prefs(app).apply { hostToken = "trusted"; checkUpdates = false }
        val controller = Robolectric.buildActivity(MainActivity::class.java, Intent().putExtra("token", "untrusted")).create()
        try {
            assertEquals("trusted", prefs.hostToken)
            controller.newIntent(Intent().putExtra("token", "untrusted-again"))
            assertEquals("trusted", prefs.hostToken)
        } finally { controller.destroy() }
    }

    @Test fun t039_tokenDeliveryRequiresShellPermissionAndFrontsLauncher() {
        val component = ComponentName(app.packageName, "com.uscreen.TokenActivity")
        val info = app.packageManager.getActivityInfo(component, 0)
        assertEquals("android.permission.DUMP", info.permission)
        assertTrue(info.exported)
        val token = "a".repeat(64)
        val controller = Robolectric.buildActivity(TokenActivity::class.java,
            Intent().setComponent(component).putExtra("token", token)).create()
        try {
            assertEquals(token, Prefs(app).hostToken)
            val next = org.robolectric.Shadows.shadowOf(controller.get()).nextStartedActivity
            assertEquals(MainActivity::class.java.name, next.component?.className)
            assertFalse(next.hasExtra("token"))
        } finally { controller.destroy() }
    }

    @Test fun t040_backgroundActivityDisablesTiltSensor() {
        Prefs(app).checkUpdates = false
        val controller = Robolectric.buildActivity(MainActivity::class.java).create()
        val activity = controller.get()
        // Avoid network work; exercise the real activity lifecycle with a recording sensor.
        set(activity, "touchCapture", null)
        set(activity, "videoReceiver", null)
        controller.start()
        var disabled = false
        set(activity, "tiltListener", object : OrientationEventListener(activity) {
            override fun onOrientationChanged(orientation: Int) {}
            override fun disable() { disabled = true }
        })
        controller.stop()
        try { assertTrue("onStop must unregister the sensor", disabled) }
        finally { controller.destroy() }
    }

    @Test fun t041_oneMbpsSettingMatchesHostMinimum() {
        val prefs = Prefs(app)
        prefs.bitrateKbps = 1000
        assertEquals(1000, prefs.bitrateKbps)
    }

    @Test fun t042_controlSocketPingsDetectDeadLinks() {
        val capture = TouchCapture()
        assertEquals(5000, (get(capture, "client") as OkHttpClient).pingIntervalMillis)
    }

    @Test fun t112_replacedAndDisconnectedCallbacksCannotResurrectControl() {
        val capture = TouchCapture()
        val old = Socket()
        val current = Socket()
        val listener = get(capture, "wsListener") as WebSocketListener
        var callbacks = 0
        capture.token = "a".repeat(64)
        capture.sendConfig(2000, 30)
        capture.onModeKnown = { callbacks++ }
        capture.onCodecKnown = { callbacks++ }
        set(capture, "webSocket", current)
        val response = Response.Builder().request(old.request()).protocol(Protocol.HTTP_1_1).code(101).message("Switching Protocols").build()
        listener.onOpen(old, response)
        listener.onMessage(old, """{"pen_only":true,"codec":"hevc"}""")
        assertFalse(capture.isControlConnected())
        assertEquals(0, callbacks)
        assertTrue(old.messages.isEmpty())
        listener.onOpen(current, response)
        assertTrue(capture.isControlConnected())
        assertTrue(current.messages.isNotEmpty())
        capture.disconnect()
        current.messages.clear()
        listener.onOpen(current, response)
        listener.onMessage(current, """{"pen_only":false,"codec":"h264"}""")
        assertFalse(capture.isControlConnected())
        assertEquals(0, callbacks)
        assertTrue(current.messages.isEmpty())
    }

    @Test fun t112_closedSocketCannotOpenAgain() {
        for (failed in listOf(false, true)) {
            val capture = TouchCapture()
            val socket = Socket()
            val listener = get(capture, "wsListener") as WebSocketListener
            val response = Response.Builder().request(socket.request()).protocol(Protocol.HTTP_1_1).code(101).message("Switching Protocols").build()
            set(capture, "webSocket", socket)
            listener.onOpen(socket, response)
            if (failed) listener.onFailure(socket, java.io.IOException("closed"), null)
            else listener.onClosed(socket, 1000, "closed")
            listener.onOpen(socket, response)
            try { assertFalse(capture.isControlConnected()) }
            finally { capture.disconnect() }
        }
    }

    @Test fun t112_backgroundModeAndCodecCallbacksCannotStartVideo() {
        Prefs(app).checkUpdates = false
        val controller = Robolectric.buildActivity(MainActivity::class.java).create()
        val activity = controller.get()
        val capture = get(activity, "touchCapture") as TouchCapture
        val receiver = get(activity, "videoReceiver") as VideoReceiver
        try {
            assertFalse(get(activity, "started") as Boolean)
            capture.onModeKnown?.invoke(false)
            capture.onCodecKnown?.invoke("hevc")
            org.robolectric.Shadows.shadowOf(android.os.Looper.getMainLooper()).idle()
            assertFalse("Background callback started video", get(receiver, "isRunning") as Boolean)
        } finally { controller.destroy() }
    }

    @Test @org.robolectric.annotation.LooperMode(org.robolectric.annotation.LooperMode.Mode.PAUSED)
    fun t112_queuedGreetingCannotAffectReplacementSession() {
        Prefs(app).checkUpdates = false
        val controller = Robolectric.buildActivity(MainActivity::class.java).create()
        val activity = controller.get()
        val capture = get(activity, "touchCapture") as TouchCapture
        val receiver = get(activity, "videoReceiver") as VideoReceiver
        try {
            set(activity, "started", true)
            val worker = Thread { capture.onModeKnown?.invoke(false); capture.onCodecKnown?.invoke("hevc") }
            worker.start()
            worker.join()
            capture.disconnect()
            org.robolectric.Shadows.shadowOf(android.os.Looper.getMainLooper()).idle()
            assertFalse("Queued old greeting started video", get(receiver, "isRunning") as Boolean)
            assertEquals(VideoReceiver.MIME_TYPE, receiver.mimeType)
        } finally { set(activity, "started", false); controller.destroy() }
    }

    private class Socket : WebSocket {
        val messages = mutableListOf<String>()
        override fun request() = Request.Builder().url(TouchCapture.WS_URL).build()
        override fun queueSize() = 0L
        override fun send(text: String): Boolean { messages.add(text); return true }
        override fun send(bytes: ByteString) = false
        override fun close(code: Int, reason: String?) = true
        override fun cancel() {}
    }

    private fun event(tool: Int, action: Int): MotionEvent = MotionEvent.obtain(
        0, 10, action, 1,
        arrayOf(MotionEvent.PointerProperties().apply { id = 0; toolType = tool }),
        arrayOf(MotionEvent.PointerCoords().apply { x = 30f; y = 40f; pressure = 0.5f }),
        0, 0, 1f, 1f, 0, 0, 0, 0
    )

    @Test fun t062_disabledDevicesSendNoTouchPenHoverOrButtonEvents() {
        val capture = TouchCapture()
        val socket = Socket()
        set(capture, "webSocket", socket)
        set(capture, "isConnected", true)
        val listener = get(capture, "wsListener") as WebSocketListener
        listener.onMessage(socket, """{"touch":false,"pen":false,"pen_only":false}""")
        for (tool in listOf(MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)) {
            for (action in listOf(MotionEvent.ACTION_DOWN, MotionEvent.ACTION_MOVE, MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL, MotionEvent.ACTION_HOVER_MOVE, MotionEvent.ACTION_HOVER_EXIT, MotionEvent.ACTION_BUTTON_PRESS)) {
                val e = event(tool, action)
                capture.handleMotionEvent(e, 100, 100)
                capture.handleHoverEvent(e, 100, 100)
                e.recycle()
            }
        }
        assertTrue("Disabled input must not generate packets: ${socket.messages}", socket.messages.isEmpty())
        listener.onMessage(socket, """{"touch":true,"pen":true}""")
        val e = event(MotionEvent.TOOL_TYPE_FINGER, MotionEvent.ACTION_DOWN)
        capture.handleMotionEvent(e, 100, 100)
        e.recycle()
        assertEquals(1, socket.messages.size)
    }
}
