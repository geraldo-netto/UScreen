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

    @Test fun t377_recompositionAndLocalSettingsPreserveReceiverOwnership() {
        val receiver = VideoReceiver { error("T377 must not open a socket") }
        val prefs = Prefs(org.robolectric.RuntimeEnvironment.getApplication())
        val session = SessionCoordinator(prefs, { it() }, receiver, null)
        receiver.start() // No Surface: this owns a job, without network or decoder work.
        val jobField = VideoReceiver::class.java.getDeclaredField("job").apply { isAccessible = true }
        val original = jobField.get(receiver)
        compose.setContent { UScreenTheme {
            UScreenMain({}, presentation = session.presentation, settings = session.settings,
                onSettingsEvent = session::handle)
        } }
        try {
            compose.onNodeWithText("⚙").performClick()
            compose.runOnIdle {
                session.handle(SettingsEvent.ShowStats(true))
                session.handle(SettingsEvent.Brightness(65))
                session.handle(SettingsEvent.CheckUpdates(false))
            }
            compose.onNodeWithText("Brightness: 65%").performScrollTo().assertIsDisplayed()
            compose.runOnIdle {
                assertSame("T377: rendering restarted the receiver", original, jobField.get(receiver))
                assertFalse(prefs.hasUserSettings)
                assertEquals(60, receiver.streamFps)
            }
        } finally { receiver.stop() }
    }

    @Test fun t349_displayDefaultsCanBeChangedWhileStreaming() = checkDisplayControls(false)
    @Test fun t349_displayDefaultsCanBeChangedInPenMode() = checkDisplayControls(true)

    private fun checkDisplayControls(penOnly: Boolean) {
        val app = org.robolectric.RuntimeEnvironment.getApplication()
        val prefs = Prefs(app)
        var changes = 0
        val session = SessionCoordinator(prefs, { it() }, null, null)
        compose.setContent { UScreenTheme {
            UScreenMain({}, penOnly = penOnly, settings = session.settings,
                displayRefreshRates = listOf(60f, 120f), onSettingsEvent = { session.handle(it); changes++ })
        } }
        compose.onNodeWithText("⚙").performClick()
        compose.onNodeWithText("Brightness: 50%").performScrollTo().assertIsDisplayed()
        compose.onNodeWithContentDescription("Brightness").performScrollTo()
            .performSemanticsAction(androidx.compose.ui.semantics.SemanticsActions.SetProgress) { it(75f) }
        compose.onNodeWithText("Brightness: 75%").assertIsDisplayed()
        compose.onNodeWithText("60 Hz").performScrollTo().assertIsSelected()
        compose.onNodeWithText("System default").performScrollTo().performClick().assertIsSelected()
        compose.runOnIdle {
            val saved = app.getSharedPreferences("uscreen", android.content.Context.MODE_PRIVATE)
            assertEquals(75, saved.getInt("brightness_percent", -1))
            assertEquals(0f, saved.getFloat("display_refresh_rate", -1f), 0f)
            assertFalse("T349 local display controls must not replace host stream settings", prefs.hasUserSettings)
            assertEquals(60, prefs.fps)
        }
        compose.onNodeWithText("60 Hz").performScrollTo().performClick().assertIsSelected()
        compose.runOnIdle {
            val saved = app.getSharedPreferences("uscreen", android.content.Context.MODE_PRIVATE)
            assertEquals(60f, saved.getFloat("display_refresh_rate", -1f), 0f)
        }
        compose.onNodeWithText("120 Hz").performScrollTo().performClick().assertIsSelected()
        compose.runOnIdle {
            assertEquals("T349 every change must apply immediately", 4, changes)
            assertEquals(75, Prefs(app).brightnessPercent)
            assertEquals(120f, Prefs(app).displayRefreshRate, 0f)
        }
    }

    @Test fun t090_shortLandscapeWithLargeFontsCanScrollToApply() {
        var applied = false
        var dismissed = false
        compose.setContent {
            MaterialTheme {
                CompositionLocalProvider(LocalDensity provides Density(1f, 1.8f)) {
                    SettingsSheet(SettingsValues(), null, {}, false, { dismissed = true },
                        onSettingsEvent = { if (it is SettingsEvent.Stream) applied = true })
                }
            }
        }
        compose.onNodeWithText("Apply").performScrollTo().assertIsDisplayed().performClick()
        compose.runOnIdle { assertTrue(applied); assertTrue(dismissed) }
    }

    @Test fun t337_githubLinkWithoutBrowserKeepsScreenAlive() = checkWebLinks(false, "Open GitHub")
    @Test fun t337_updatePillWithoutBrowserKeepsScreenAlive() = checkWebLinks(false, "Update 9.9.9 available")
    @Test fun t337_settingsUpdateWithoutBrowserKeepsScreenAlive() = checkWebLinks(false, "Update available: 9.9.9")
    @Test fun t337_browserReceivesAllForkLinks() = checkWebLinks(true,
        "Open GitHub", "Update 9.9.9 available", "Update available: 9.9.9")

    private fun checkWebLinks(browserAvailable: Boolean, vararg labels: String) {
        val app = org.robolectric.RuntimeEnvironment.getApplication()
        val shadow = org.robolectric.Shadows.shadowOf(app)
        var dismissed = false
        compose.setContent { UScreenTheme {
            UScreenMain({}, updateAvailable = "9.9.9", showThanks = true,
                onDismissThanks = { dismissed = true })
        } }
        shadow.checkActivities(true)
        try {
            for (label in labels) {
                val url = if (label == "Open GitHub") "https://github.com/geraldo-netto/UScreen"
                          else UpdateCheck.RELEASES_PAGE
                val intent = android.content.Intent(android.content.Intent.ACTION_VIEW, android.net.Uri.parse(url))
                if (browserAvailable) registerBrowser(app, intent)
                if (label.startsWith("Update available:")) {
                    compose.onNodeWithText("⚙").performClick()
                    compose.onNodeWithText(label).performScrollTo()
                }
                compose.onNodeWithText(label).performClick()
                compose.runOnIdle {
                    if (browserAvailable) {
                        val launched = shadow.nextStartedActivity
                        assertNotNull("T337: browser was not launched", launched)
                        assertEquals(android.content.Intent.ACTION_VIEW, launched.action)
                        assertEquals(url, launched.dataString)
                    } else {
                        assertEquals("No browser available to open this link.",
                            org.robolectric.shadows.ShadowToast.getTextOfLatestToast())
                    }
                }
            }
            if ("Open GitHub" in labels) assertEquals(browserAvailable, dismissed)
        } finally { shadow.checkActivities(false) }
    }

    private fun registerBrowser(app: android.app.Application, intent: android.content.Intent) {
        val activity = android.content.pm.ActivityInfo().apply {
            packageName = "fixture.browser"
            name = "BrowserActivity"
            exported = true
            applicationInfo = android.content.pm.ApplicationInfo().apply {
                packageName = "fixture.browser"
                enabled = true
            }
        }
        org.robolectric.Shadows.shadowOf(app.packageManager).addResolveInfoForIntent(intent,
            android.content.pm.ResolveInfo().apply { activityInfo = activity })
    }

    private val draw = "Draw here — it goes to the screen on your computer."

    @Test fun t300_queuedVideoCallbacksCannotUndoNewerConnectionState() {
        val receiver = VideoReceiver()
        val running = VideoReceiver::class.java.getDeclaredField("isRunning").apply { isAccessible = true }
        val presentation = StreamPresentation(receiver, null)
        compose.setContent { UScreenTheme { UScreenMain({}, presentation = presentation) } }
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
        val presentation = StreamPresentation(receiver, null)
        compose.setContent { UScreenTheme { UScreenMain({}, presentation = presentation) } }
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
        ControlSession::class.java.getDeclaredField("webSocket").apply { isAccessible = true }.set(capture.control, socket)
    }

    @Test fun t241_t247_penModeRequiresAuthenticatedControlAndRecoversWithoutVideo() {
        // T247: the Rust suite verifies this resource against InputResponse serialization.
        val greeting = javaClass.getResource("/control-connected.json")!!.readText()
        val capture = TouchCapture()
        val receiver = VideoReceiver()
        val listener = ControlSession::class.java.getDeclaredField("wsListener")
            .apply { isAccessible = true }.get(capture.control) as WebSocketListener
        val old = Socket()
        val fresh = Socket()
        val response = Response.Builder().request(old.request()).protocol(Protocol.HTTP_1_1)
            .code(101).message("Switching Protocols").build()
        val presentation = StreamPresentation(receiver, capture)
        compose.setContent { UScreenTheme { UScreenMain({}, penOnly = true, presentation = presentation) } }
        try {
            compose.onNodeWithText(draw).assertDoesNotExist()
            compose.runOnIdle { install(capture, old); listener.onOpen(old, response) }
            compose.onNodeWithText(draw).assertDoesNotExist() // WebSocket open is not authentication.
            compose.runOnIdle { listener.onMessage(old, greeting) }
            compose.onNodeWithText(draw).assertIsDisplayed()
            compose.runOnIdle { listener.onFailure(old, java.io.IOException("USB detached"), null) }
            compose.onNodeWithText(draw).assertDoesNotExist()
            compose.onNodeWithText("Reconnecting to the host…").assertIsDisplayed()
            compose.runOnIdle {
                install(capture, fresh)
                listener.onOpen(fresh, response)
                listener.onMessage(old, greeting)
            }
            compose.onNodeWithText(draw).assertDoesNotExist()
            compose.runOnIdle {
                listener.onMessage(fresh, greeting)
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
