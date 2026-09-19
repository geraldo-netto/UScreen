package com.uscreen

import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createComposeRule
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

    @Test fun t509_modeAndOrientationSwitchesHaveDescriptiveAccessibleActions() {
        val events = mutableListOf<SettingsEvent>()
        compose.setContent { UScreenTheme {
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
