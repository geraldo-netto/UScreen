package com.blent

import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.unit.dp
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.LooperMode

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34], qualifiers = "w960dp-h600dp-land")
@LooperMode(LooperMode.Mode.PAUSED)
class SettingsAccessibilityTest {
    @get:Rule val compose = createComposeRule()

    @Test fun t611_settingsButtonHasAccessibleNameAndMinimumTouchTarget() {
        compose.setContent { BlentTheme { BlentMain({}) } }
        compose.onNodeWithContentDescription("Open settings")
            .assertHasClickAction().assertWidthIsAtLeast(48.dp).assertHeightIsAtLeast(48.dp)
            .performClick()
        compose.onNodeWithText("Settings").assertIsDisplayed()
    }

    @Test fun t611_advancedControlsDiscloseWithoutApplyingDraftOrStartingCamera() {
        val events = mutableListOf<SettingsEvent>()
        compose.setContent { BlentTheme {
            SettingsSheet(SettingsValues(), null, {}, false, {}, onSettingsEvent = events::add)
        } }
        compose.onNodeWithText("Bitrate:", substring = true).assertDoesNotExist()
        compose.onNodeWithContentDescription("Show stats overlay").assertDoesNotExist()
        compose.onNodeWithText("Advanced video settings").performScrollTo().performClick()
        compose.onNodeWithText("Bitrate:", substring = true).performScrollTo().assertIsDisplayed()
        compose.onNodeWithText("App & diagnostics").performScrollTo().performClick()
        compose.onNodeWithContentDescription("Show stats overlay").performScrollTo().assertIsDisplayed()
        compose.runOnIdle { assertTrue(events.isEmpty()) }
        compose.onNodeWithText("Advanced video settings").performScrollTo().performClick()
        compose.onNodeWithText("Bitrate:", substring = true).assertDoesNotExist()
    }

    @Test fun t509_modeAndOrientationSwitchesHaveDescriptiveAccessibleActions() {
        val events = mutableListOf<SettingsEvent>()
        compose.setContent { BlentTheme {
            SettingsSheet(SettingsValues(), null, {}, false, {}, onSettingsEvent = events::add)
        } }
        compose.onNodeWithContentDescription("Rotate automatically")
            .performScrollTo().assertIsOn().performClick()
        compose.runOnIdle { assertEquals(SettingsEvent.Orientation(Prefs.ORIENTATION_CAMERA_DOWN), events.single()) }
        compose.onNodeWithContentDescription("Graphics tablet")
            .performScrollTo().assertIsOff().performClick()
        compose.runOnIdle { assertEquals(SettingsEvent.Mode(true), events.last()) }
    }
}
