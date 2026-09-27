// Copyright (c) 2026 Geraldo Netto
package com.blent

import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createComposeRule
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.LooperMode

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34], qualifiers = "w960dp-h600dp-land")
@LooperMode(LooperMode.Mode.PAUSED)
class DecoderDiagnosticsUiTest {
    @get:Rule val compose = createComposeRule()

    @Test fun t417_settingsShowInactiveAndChangingReadOnlyDecoderState() {
        val state = mutableStateOf(DecoderDiagnostics())
        compose.setContent { BlentTheme {
            SettingsSheet(SettingsValues(), null, {}, false, {}, decoderDiagnostics = state.value)
        } }
        compose.onNodeWithText("App & diagnostics").performScrollTo().performClick()
        compose.onNodeWithText("No active decoder").performScrollTo().assertIsDisplayed()
        compose.runOnIdle {
            state.value = DecoderDiagnostics(ActiveDecoderDiagnostics("test.avc", "video/avc", 640, 480, 30,
                true, false, null, listOf("Operating rate=60")))
        }
        compose.onNodeWithText("Active codec: test.avc").performScrollTo().assertIsDisplayed()
        compose.onNodeWithText("Hardware accelerated: Yes").performScrollTo().assertIsDisplayed()
        compose.onNodeWithText("Advertised standard low latency: No").performScrollTo().assertIsDisplayed()
        compose.onNodeWithText("Advertised 2× rate headroom at this size: Unknown").performScrollTo().assertIsDisplayed()
        compose.onNodeWithText("Requested settings: Operating rate=60").performScrollTo().assertIsDisplayed()
        compose.runOnIdle { state.value = state.value.copy(active = state.value.active!!.copy(name = null, requested = emptyList())) }
        compose.onNodeWithText("Active codec: Unknown").performScrollTo().assertIsDisplayed()
        compose.onNodeWithText("Requested settings: None").performScrollTo().assertIsDisplayed()
        compose.runOnIdle { state.value = DecoderDiagnostics(watchdogFallback = true) }
        compose.onNodeWithText("No active decoder").performScrollTo().assertIsDisplayed()
        compose.onNodeWithText("Watchdog fallback:", substring = true).performScrollTo().assertIsDisplayed()
        compose.onNodeWithText("Effective hints are unknown", substring = true).performScrollTo().assertIsDisplayed()
    }
}
