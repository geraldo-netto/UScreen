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

    @Test fun t332_newRequestBeforeUiCallbackCaptureStillWins() {
        lateinit var listener: WebSocketListener
        val socket = Socket()
        val capture = TouchCapture(WebSocket.Factory { _, created -> listener = created; socket })
        val prefs = Prefs(org.robolectric.RuntimeEnvironment.getApplication())
        val session = SessionCoordinator(prefs, { it() }, null, capture)
        val callback = capture.onSettingsRejected!!
        capture.onSettingsRejected = { rejected ->
            session.handle(SettingsEvent.Stream(15000, 30))
            callback(rejected)
        }
        try {
            session.start()
            listener.onOpen(socket, Response.Builder().request(socket.request()).protocol(Protocol.HTTP_1_1)
                .code(101).message("test").build())
            session.handle(SettingsEvent.Stream(25000, 90))
            listener.onMessage(socket, javaClass.getResource("/settings-rejected.json")!!.readText())
            assertEquals("T332: response ownership must come from the control request", 30, prefs.fps)
            assertEquals(15000, prefs.bitrateKbps)
        } finally { session.stop() }
    }

    @Test fun t332_queuedRejectionCannotRollBackANewerUiEdit() {
        lateinit var listener: WebSocketListener
        val socket = Socket()
        val capture = TouchCapture(WebSocket.Factory { _, created -> listener = created; socket })
        val prefs = Prefs(org.robolectric.RuntimeEnvironment.getApplication())
        val pending = mutableListOf<() -> Unit>()
        val receiver = VideoReceiver { error("T332 must not open video") }
        val session = SessionCoordinator(prefs, { pending.add(it) }, receiver, capture)
        try {
            session.start()
            listener.onOpen(socket, Response.Builder().request(socket.request()).protocol(Protocol.HTTP_1_1)
                .code(101).message("test").build())
            session.handle(SettingsEvent.Stream(25000, 90))
            listener.onMessage(socket, javaClass.getResource("/settings-rejected.json")!!.readText())
            session.handle(SettingsEvent.Stream(15000, 30))
            pending.toList().forEach { it() }
            assertEquals("T332: queued rejection replaced newer UI edit", 30, prefs.fps)
            assertEquals(15000, prefs.bitrateKbps)
            assertEquals(30, receiver.streamFps)
        } finally { session.stop() }
    }

    @Test fun t332_rejectedStreamSettingsRestoreConfirmedValuesAndShowReason() {
        lateinit var listener: WebSocketListener
        val socket = Socket()
        val capture = TouchCapture(WebSocket.Factory { _, created -> listener = created; socket })
        val prefs = Prefs(org.robolectric.RuntimeEnvironment.getApplication())
        prefs.fps = 60; prefs.bitrateKbps = 20000
        val session = SessionCoordinator(prefs, { it() }, null, capture)
        compose.setContent { UScreenTheme {
            SettingsSheet(session.settings, null, {}, false, {}, onSettingsEvent = session::handle)
        } }
        try {
            compose.runOnIdle {
                session.start()
                listener.onOpen(socket, Response.Builder().request(socket.request()).protocol(Protocol.HTTP_1_1)
                    .code(101).message("test").build())
                session.handle(SettingsEvent.Stream(25000, 90))
                listener.onMessage(socket, javaClass.getResource("/settings-rejected.json")!!.readText())
                assertEquals("T332: rejected FPS was retained", 60, prefs.fps)
                assertEquals(20000, prefs.bitrateKbps)
                assertEquals(60, session.settings.fps)
            }
            compose.onNodeWithText("Settings rejected:", substring = true).performScrollTo().assertIsDisplayed()
            compose.onNodeWithText("Bitrate: 20 Mbps").performScrollTo().assertIsDisplayed()
        } finally { session.stop() }
    }

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

    @Test fun t388_battery_toggle_is_opt_in_persistent_and_preserves_display_and_stream() {
        val app = org.robolectric.RuntimeEnvironment.getApplication()
        val prefs = Prefs(app)
        val saved = app.getSharedPreferences("uscreen", android.content.Context.MODE_PRIVATE)
        assertFalse(saved.getBoolean("battery_saver", false))
        prefs.brightnessPercent = 75; prefs.displayRefreshRate = 120f
        prefs.fps = 90; prefs.bitrateKbps = 40000
        val session = SessionCoordinator(prefs, { it() }, null, null)
        compose.setContent { UScreenTheme {
            SettingsSheet(session.settings, null, {}, false, {}, onSettingsEvent = session::handle)
        } }
        val toggle = compose.onNodeWithContentDescription("Battery saver")
        toggle.performScrollTo().assertIsOff().performClick().assertIsOn()
        compose.runOnIdle {
            assertTrue(saved.getBoolean("battery_saver", false))
            val reloaded = SettingsValues.read(Prefs(app))
            assertEquals(75, reloaded.brightness); assertEquals(120f, reloaded.refreshRate, 0f)
            assertEquals(90, reloaded.fps); assertEquals(40000, reloaded.bitrateKbps)
            assertFalse(prefs.hasUserSettings)
        }
        toggle.performClick().assertIsOff()
        compose.runOnIdle { assertFalse(saved.getBoolean("battery_saver", true)) }
    }

    @Test fun t388_pen_only_power_requires_live_host_acknowledgement() {
        lateinit var listener: WebSocketListener
        val socket = Socket()
        val capture = TouchCapture(WebSocket.Factory { _, value -> listener = value; socket })
        val prefs = Prefs(org.robolectric.RuntimeEnvironment.getApplication()).apply { batterySaver = true }
        val session = SessionCoordinator(prefs, { it() }, null, capture)
        session.start()
        try {
            assertFalse(session.powerNow().active)
            listener.onOpen(socket, Response.Builder().request(socket.request()).protocol(Protocol.HTTP_1_1)
                .code(101).message("Switching Protocols").build())
            listener.onMessage(socket, """{"status":"connected","transport":"usb","pen_only":true}""")
            assertEquals(StreamingPower(true, true, StreamTransport.USB), session.powerNow())
            session.handle(SettingsEvent.BatterySaver(false))
            assertEquals(StreamingPower(false, true, StreamTransport.USB), session.powerNow())
            assertFalse(prefs.hasUserSettings)
            session.stop()
            assertFalse(session.powerNow().active)
            assertEquals(StreamTransport.UNKNOWN, session.powerNow().transport)
        } finally { session.stop() }
    }

    @Test fun t388_hidden_stats_do_not_schedule_presentation_samples() {
        val receiver = VideoReceiver { error("T388: UI test must not connect") }
        val presentation = StreamPresentation(receiver, null)
        val field = VideoReceiver::class.java.getDeclaredField("statistics").apply { isAccessible = true }
        val statistics = field.get(receiver) as ReceiverStatistics
        repeat(60) { statistics.frameRendered() }
        statistics.sample(System.nanoTime() + 1_000_000_000L)
        assertTrue(statistics.fps > 0f)
        val visible = androidx.compose.runtime.mutableStateOf(false)
        compose.setContent { UScreenTheme {
            UScreenMain({}, presentation = presentation, settings = SettingsValues(showStats = visible.value))
        } }
        compose.runOnIdle { receiver.onConnected!!.invoke(); org.robolectric.Shadows.shadowOf(android.os.Looper.getMainLooper()).idle() }
        compose.waitForIdle()
        compose.mainClock.autoAdvance = false
        compose.mainClock.advanceTimeBy(2100)
        compose.runOnIdle { assertEquals("T388: hidden stats still sampled", 0f, presentation.fps, 0f) }
        compose.runOnIdle { assertTrue("T388: connection fixture retired", presentation.connected); visible.value = true }
        compose.mainClock.autoAdvance = true
        compose.waitForIdle()
        compose.mainClock.autoAdvance = false
        compose.mainClock.advanceTimeBy(1100)
        compose.runOnIdle { assertTrue("T388: visible stats stopped updating", presentation.fps > 0f) }
        compose.runOnIdle { visible.value = false }
        compose.mainClock.autoAdvance = true
        compose.waitForIdle()
        compose.mainClock.autoAdvance = false
        val previous = presentation.fps
        compose.runOnIdle { statistics.sample(System.nanoTime() + 2_000_000_000L) }
        compose.mainClock.advanceTimeBy(2100)
        compose.runOnIdle { assertEquals("T388: hiding stats did not cancel sampling", previous, presentation.fps, 0f) }
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
        val component = android.content.ComponentName("fixture.browser", "BrowserActivity")
        val packages = org.robolectric.Shadows.shadowOf(app.packageManager)
        packages.addActivityIfNotPresent(component).apply {
            exported = true
            enabled = true
            applicationInfo.enabled = true
        }
        packages.addIntentFilterForActivity(component,
            android.content.IntentFilter(checkNotNull(intent.action)).apply {
                addCategory(android.content.Intent.CATEGORY_DEFAULT)
                addDataScheme(checkNotNull(intent.scheme))
            })
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
