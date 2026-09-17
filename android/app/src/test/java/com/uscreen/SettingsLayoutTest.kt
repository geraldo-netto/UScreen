package com.uscreen

import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.unit.Density
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
@Config(sdk = [27, 34], qualifiers = "w960dp-h360dp-land")
@LooperMode(LooperMode.Mode.PAUSED)
class SettingsLayoutTest {
    @get:Rule val compose = createComposeRule()

    @Test fun t090_shortLandscapeWithLargeFontsCanScrollToApply() {
        var applied = false
        var dismissed = false
        compose.setContent {
            MaterialTheme {
                CompositionLocalProvider(LocalDensity provides Density(1f, 1.8f)) {
                    SettingsSheet(null, null, {}, false, {}, false, {}, false, {},
                        Prefs.ORIENTATION_AUTO, {},
                        { _, _ -> applied = true }, { dismissed = true })
                }
            }
        }
        compose.onNodeWithText("Apply").performScrollTo().assertIsDisplayed().performClick()
        compose.runOnIdle { assertTrue(applied); assertTrue(dismissed) }
    }

    private val draw = "Draw here — it goes to the screen on your computer."

    @Test fun t300_queuedVideoCallbacksCannotUndoNewerConnectionState() {
        val receiver = VideoReceiver()
        val running = VideoReceiver::class.java.getDeclaredField("isRunning").apply { isAccessible = true }
        compose.setContent { UScreenTheme { UScreenMain({}, videoReceiver = receiver) } }
        try {
            compose.onNodeWithText("Waiting for the host…").assertIsDisplayed()
            compose.runOnIdle {
                running.set(receiver, true)
                // Joining on the UI thread leaves the worker's UI update queued.
                Thread { receiver.onConnected!!.invoke() }.apply { start(); join(3000); assertFalse(isAlive) }
                receiver.stop()
            }
            compose.onNodeWithText("Waiting for the host…").assertIsDisplayed()
            compose.runOnIdle {
                running.set(receiver, true)
                receiver.onConnected!!.invoke()
            }
            compose.onNodeWithText("Waiting for the host…").assertDoesNotExist()
            compose.runOnIdle {
                Thread { receiver.onDisconnected!!.invoke() }.apply { start(); join(3000); assertFalse(isAlive) }
                receiver.onConnected!!.invoke()
            }
            compose.onNodeWithText("Waiting for the host…").assertDoesNotExist()
            compose.runOnIdle { receiver.stop() }
            compose.onNodeWithText("Waiting for the host…").assertIsDisplayed()
        } finally { receiver.stop() }
    }

    @Test fun t248_stoppingVideoRestoresTheWaitingScreen() {
        val receiver = VideoReceiver()
        compose.setContent { UScreenTheme { UScreenMain({}, videoReceiver = receiver) } }
        try {
            compose.onNodeWithText("Waiting for the host…").assertIsDisplayed()
            compose.runOnIdle {
                VideoReceiver::class.java.getDeclaredField("isRunning").apply { isAccessible = true }.set(receiver, true)
                receiver.onConnected!!.invoke()
            }
            compose.onNodeWithText("Waiting for the host…").assertDoesNotExist()
            compose.runOnIdle { receiver.stop() }
            compose.onNodeWithText("Waiting for the host…").assertIsDisplayed()
        } finally { receiver.stop() }
    }

    private class Socket : WebSocket {
        override fun request() = Request.Builder().url(TouchCapture.WS_URL).build()
        override fun queueSize() = 0L
        override fun send(text: String) = true
        override fun send(bytes: ByteString) = true
        override fun close(code: Int, reason: String?) = true
        override fun cancel() {}
    }

    private fun install(capture: TouchCapture, socket: WebSocket) {
        TouchCapture::class.java.getDeclaredField("webSocket").apply { isAccessible = true }.set(capture, socket)
    }

    @Test fun t241_penModeRequiresAuthenticatedControlAndRecoversWithoutVideo() {
        val capture = TouchCapture()
        val receiver = VideoReceiver()
        val listener = TouchCapture::class.java.getDeclaredField("wsListener")
            .apply { isAccessible = true }.get(capture) as WebSocketListener
        val old = Socket()
        val fresh = Socket()
        val response = Response.Builder().request(old.request()).protocol(Protocol.HTTP_1_1)
            .code(101).message("Switching Protocols").build()
        compose.setContent { UScreenTheme { UScreenMain({}, penOnly = true, touchCapture = capture, videoReceiver = receiver) } }
        try {
            compose.onNodeWithText(draw).assertDoesNotExist()
            compose.runOnIdle { install(capture, old); listener.onOpen(old, response) }
            compose.onNodeWithText(draw).assertDoesNotExist() // WebSocket open is not authentication.
            compose.runOnIdle { listener.onMessage(old, """{"type":"connected","pen_only":true}""") }
            compose.onNodeWithText(draw).assertIsDisplayed()
            compose.runOnIdle { listener.onFailure(old, java.io.IOException("USB detached"), null) }
            compose.onNodeWithText(draw).assertDoesNotExist()
            compose.onNodeWithText("Reconnecting to the host…").assertIsDisplayed()
            compose.runOnIdle {
                install(capture, fresh)
                listener.onOpen(fresh, response)
                listener.onMessage(old, """{"type":"connected","pen_only":true}""")
            }
            compose.onNodeWithText(draw).assertDoesNotExist()
            compose.runOnIdle {
                listener.onMessage(fresh, """{"type":"connected","pen_only":true}""")
                listener.onClosed(old, 1000, "late old close")
            }
            compose.onNodeWithText(draw).assertIsDisplayed()
            compose.runOnIdle { capture.disconnect() }
            compose.onNodeWithText(draw).assertDoesNotExist()
            val running = VideoReceiver::class.java.getDeclaredField("isRunning").apply { isAccessible = true }
            assertFalse(running.get(receiver) as Boolean)
        } finally { capture.disconnect(); receiver.stop() }
    }
}
