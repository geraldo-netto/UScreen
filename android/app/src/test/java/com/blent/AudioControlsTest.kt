package com.blent

import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.semantics.SemanticsActions
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.MutableStateFlow
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.*
import org.robolectric.annotation.Config
import org.robolectric.annotation.LooperMode

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34], qualifiers = "w960dp-h600dp-land", instrumentedPackages = ["androidx.compose.ui.platform"])
@LooperMode(LooperMode.Mode.PAUSED)
class AudioControlsTest {
    @get:Rule val compose = createComposeRule()
    @Test fun t718_settingsArePassiveAndStopIsIndependent() {
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
        val invitations = MutableStateFlow<AudioEndpoint?>(null)
        var opened = 0; var permission = false
        val binding = AudioBinding(RuntimeEnvironment.getApplication(), {}, { permission },
            { _, _, _, _ -> opened++; awaitCancellation() }, invitations, scope)
        binding.start()
        compose.setContent { BlentTheme { SettingsSheet(SettingsValues(), null, {}, false, {}, onSettingsEvent = {}, cameraControls = { AudioControls(binding) }) } }
        try {
            compose.onNodeWithText("Allow microphone").performScrollTo().assertIsNotEnabled()
            compose.onNodeWithText("Raw").performScrollTo().performClick()
            compose.runOnIdle { assertEquals(2, binding.preferences.processing); assertEquals(0, opened) }
            compose.onNodeWithText("Speech").performScrollTo().performClick()
            compose.onNodeWithText("Use host setting").performScrollTo().performClick()
            compose.onNodeWithContentDescription("Use built-in microphone").performScrollTo().performClick()
            compose.onNodeWithContentDescription("Allow background microphone").performScrollTo().performClick()
            compose.onAllNodes(SemanticsMatcher.keyIsDefined(SemanticsActions.SetProgress)).onLast().performScrollTo().performSemanticsAction(SemanticsActions.SetProgress) { it(150f) }
            compose.runOnIdle { assertEquals(150, binding.preferences.gain); assertEquals(0, opened)
                invitations.value = AudioEndpoint("a".repeat(64), 12345, 1, 1, 40, false); permission = true }
            compose.onNodeWithText("Allow microphone").performScrollTo().performClick()
            compose.runOnIdle { assertEquals(1, opened) }
            compose.onNodeWithText("Stop microphone").performScrollTo().performClick()
            compose.runOnIdle { assertFalse(binding.sharing) }
        } finally { compose.runOnIdle { binding.shutdown(); scope.cancel() } }
    }
}
