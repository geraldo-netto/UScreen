package com.blent

import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createComposeRule
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.annotation.LooperMode

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34], qualifiers = "w960dp-h600dp-land")
@LooperMode(LooperMode.Mode.PAUSED)
class SettingsControlContractTest {
    @get:Rule val compose = createComposeRule()

    @Test fun t497_displayChoicesExcludeNonfiniteNegativeAndDuplicatePanelModes() {
        val events = mutableListOf<SettingsEvent>()
        val rates = listOf(Float.NaN, Float.POSITIVE_INFINITY, Float.NEGATIVE_INFINITY, -1f, 0f, 60f, 120f, 60f)
        compose.setContent { BlentTheme {
            androidx.compose.foundation.layout.Column { DisplayControls(SettingsValues(), rates) { events.add(it) } }
        } }
        compose.onAllNodesWithText("60 Hz").assertCountEquals(1)
        compose.onAllNodesWithText("System default").assertCountEquals(1)
        compose.onNodeWithText("120 Hz").performClick()
        compose.onNodeWithText("NaN Hz").assertDoesNotExist()
        compose.onNodeWithText("-1 Hz").assertDoesNotExist()
        compose.runOnIdle { assertEquals(listOf(SettingsEvent.RefreshRate(120f)), events) }
    }

    @Test fun t497_optionalMainCallbacksPermitDismissalAndSettings() {
        val visible = androidx.compose.runtime.mutableStateOf(true)
        compose.setContent { BlentTheme {
            if (visible.value) BlentMain(onSurfaceReady = {}, showThanks = true)
        } }
        compose.onNodeWithText("Dismiss").performClick()
        compose.onNodeWithText("⚙").performClick()
        compose.onNodeWithContentDescription("Battery saver").performScrollTo().performClick()
        compose.onNodeWithText("Apply").performScrollTo().performClick()
        compose.onNodeWithText("Settings").assertDoesNotExist()
        compose.runOnIdle { visible.value = false }
        compose.waitForIdle()
        compose.onNodeWithText("Blent is working.").assertDoesNotExist()
    }

    @Test fun t497_settingsSheetAllowsAnAbsentSettingsObserver() {
        var dismissals = 0
        compose.setContent { BlentTheme {
            SettingsSheet(SettingsValues(), null, {}, false, { dismissals++ })
        } }
        compose.onNodeWithContentDescription("Battery saver").performScrollTo().performClick()
        compose.onNodeWithText("Apply").performScrollTo().performClick()
        compose.runOnIdle { assertEquals(1, dismissals) }
    }

    @Test fun t497_settingsControlsDeliverTheirUserSelectedValues() {
        val prefs = Prefs(RuntimeEnvironment.getApplication())
        val session = SessionCoordinator(prefs, { it() }, null, null)
        val events = mutableListOf<SettingsEvent>()
        var dismissals = 0
        var updates = 0
        compose.setContent { BlentTheme {
            SettingsSheet(session.settings, "v-test", { updates++ }, false, { dismissals++ },
                onSettingsEvent = { events.add(it); session.handle(it) })
        } }
        compose.onNodeWithText("Update available: v-test").performScrollTo().performClick()
        toggleBeside("Rotate automatically").performScrollTo().performClick()
        compose.onNodeWithText("Camera up").performScrollTo().performClick().assertIsSelected()
        compose.onNodeWithText("Camera down").performClick().assertIsSelected()
        toggleBeside("Rotate automatically").performScrollTo().performClick()
        compose.onNodeWithText("App & diagnostics").performScrollTo().performClick()
        compose.onNodeWithContentDescription("Show stats overlay").performScrollTo().performClick().assertIsOn()
        compose.onNodeWithContentDescription("Check for newer releases").performScrollTo().performClick().assertIsOff()
        compose.runOnIdle {
            assertEquals(1, updates)
            assertEquals(Prefs.ORIENTATION_AUTO, prefs.orientation)
            assertTrue(prefs.showStats)
            assertFalse(prefs.checkUpdates)
            assertEquals(0, dismissals)
        }
        compose.onNodeWithText("30 fps").performScrollTo().performClick().assertIsSelected()
        compose.onNodeWithText("Advanced video settings").performScrollTo().performClick()
        val bitrate = SemanticsMatcher("stream bitrate range") {
            it.config.getOrNull(SemanticsProperties.ProgressBarRangeInfo)?.range ==
                (Prefs.MIN_BITRATE_KBPS / 1000f)..(Prefs.MAX_BITRATE_KBPS / 1000f)
        }
        compose.onNode(bitrate).performScrollTo().performSemanticsAction(SemanticsActions.SetProgress) { it(60f) }
        compose.onNodeWithText("Apply").performScrollTo().performClick()
        compose.runOnIdle {
            assertTrue(events.contains(SettingsEvent.Stream(60000, 30)))
            assertEquals(60000, prefs.bitrateKbps)
            assertEquals(30, prefs.fps)
            assertEquals(1, dismissals)
        }
        toggleBeside("Graphics tablet").performScrollTo().performClick()
        compose.runOnIdle {
            assertEquals(SettingsEvent.Mode(true), events.last())
            assertEquals(2, dismissals)
        }
    }

    private fun toggleBeside(text: String) = compose.onNodeWithContentDescription(text)
}
