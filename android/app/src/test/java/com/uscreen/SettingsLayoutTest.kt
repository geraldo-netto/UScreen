package com.uscreen

import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.unit.Density
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
}
