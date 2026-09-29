package com.blent

import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createComposeRule
import okhttp3.*
import okio.ByteString
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.LooperMode

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34])
@LooperMode(LooperMode.Mode.PAUSED)
class CalibrationStartupTest {
    @get:Rule val compose = createComposeRule()
    private class Socket(val listener: WebSocketListener) : WebSocket {
        override fun request() = Request.Builder().url("ws://localhost/").build()
        override fun queueSize() = 0L
        override fun send(text: String) = true
        override fun send(bytes: ByteString) = true
        override fun close(code: Int, reason: String?) = true
        override fun cancel() {}
        fun greet(extra: String) = listener.onMessage(this,
            """{"status":"connected","codec":"h264","pen_only":false,$extra}""")
    }
    @Test fun t714_trialsKeepOneCalibrationScreenUntilFinalSelection() {
        val sockets = mutableListOf<Socket>()
        val capture = TouchCapture(WebSocket.Factory { _, listener -> Socket(listener).also { sockets.add(it) } })
        val receiver = VideoReceiver { error("T714: no physical socket") }
        val presentation = StreamPresentation(receiver, capture)
        capture.connect()
        compose.setContent { BlentTheme { BlentMain({}, presentation = presentation) } }
        try {
            compose.runOnIdle { sockets.last().greet("\"calibrating\":true") }
            compose.onNodeWithText("Optimizing display…").assertIsDisplayed()
            repeat(6) {
                compose.runOnIdle { receiver.onConnected!!.invoke() }
                compose.onNodeWithText("Optimizing display…").assertIsDisplayed()
                compose.onNodeWithText("Waiting for your computer…").assertDoesNotExist()
                compose.runOnIdle { receiver.onDisconnected!!.invoke() }
                compose.onNodeWithText("Optimizing display…").assertIsDisplayed()
            }
            compose.runOnIdle { sockets.last().greet("\"calibrating\":false"); receiver.onConnected!!.invoke() }
            compose.onNodeWithText("Optimizing display…").assertDoesNotExist()
            compose.onNodeWithText("Waiting for your computer…").assertDoesNotExist()
            compose.runOnIdle { receiver.onDisconnected!!.invoke() }
            compose.onNodeWithText("Waiting for your computer…").assertIsDisplayed()
        } finally { capture.disconnect(); receiver.stop() }
    }
    @Test fun t714_timeoutDoesNotResetOnTrialUpdatesAndDisconnectRetiresState() {
        val sockets = mutableListOf<Socket>()
        val capture = TouchCapture(WebSocket.Factory { _, listener -> Socket(listener).also { sockets.add(it) } })
        val presentation = StreamPresentation(null, capture)
        capture.connect()
        compose.setContent { BlentTheme { BlentMain({}, presentation = presentation) } }
        try {
            compose.runOnIdle { sockets.last().greet("\"calibrating\":true") }
            compose.onNodeWithText("Optimizing display…").assertIsDisplayed()
            compose.mainClock.advanceTimeBy(60_000)
            compose.runOnIdle { sockets.last().greet("\"calibrating\":true") }
            compose.mainClock.advanceTimeBy(60_500)
            compose.onNodeWithText("Display optimization timed out. Reconnect to try again.").assertIsDisplayed()
            val retired = sockets.last()
            compose.runOnIdle { capture.disconnect(); capture.connect(); retired.greet("\"calibrating\":true") }
            compose.onNodeWithText("Waiting for your computer…").assertIsDisplayed()
            compose.runOnIdle { sockets.last().greet("\"calibrating\":true") }
            compose.onNodeWithText("Optimizing display…").assertIsDisplayed()
            compose.runOnIdle { sockets.last().listener.onClosing(sockets.last(), 1000, "retired") }
            compose.onNodeWithText("Waiting for your computer…").assertIsDisplayed()
        } finally { capture.disconnect() }
    }

    @Test fun t714_malformedMetadataAndLegacyPeersCannotSuppressConnectionStatus() {
        val sockets = mutableListOf<Socket>()
        val capture = TouchCapture(WebSocket.Factory { _, listener -> Socket(listener).also { sockets.add(it) } })
        capture.connect()
        try {
            val socket = sockets.last()
            for (value in listOf("null", "-1", "1", "[]", "{}", "\"true\"", "false")) {
                socket.greet("\"calibrating\":true")
                assertTrue(capture.control.calibrating.value)
                socket.greet("\"calibrating\":$value")
                assertFalse("T714 invalid calibration metadata: $value", capture.control.calibrating.value)
            }
            capture.disconnect(); capture.connect()
            sockets.last().greet("\"touch\":true")
            assertFalse(capture.control.calibrating.value)
            assertEquals("Reconnecting to your computer…", connectionTitle(true, true, true))
        } finally { capture.disconnect() }
    }

}
