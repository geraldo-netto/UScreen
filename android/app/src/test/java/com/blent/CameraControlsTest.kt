package com.blent

import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createComposeRule
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.MutableStateFlow
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.annotation.LooperMode

@RunWith(RobolectricTestRunner::class)
// Keep Compose dispatcher caches separate from unrelated reset Robolectric loopers.
@Config(sdk = [27, 34], qualifiers = "w960dp-h600dp-land",
    instrumentedPackages = ["androidx.compose.ui.platform"])
@LooperMode(LooperMode.Mode.PAUSED)
class CameraControlsTest {
    @get:Rule val compose = createComposeRule()

    @Test fun t611_browsingAndApplyStayOffWhileExplicitStopAndRestartRespectPermission() {
        val invitations = MutableStateFlow<CameraEndpoint?>(null)
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
        var permission = true
        var requests = 0
        var opened = 0
        var closed = 0
        val events = mutableListOf<SettingsEvent>()
        val binding = CameraBinding(RuntimeEnvironment.getApplication(), { 0 }, { requests++ }, { permission },
            { _, _, _, resources -> opened++; resources.own { closed++ }; awaitCancellation() }, invitations, scope)
        binding.start()
        compose.setContent { BlentTheme {
            SettingsSheet(SettingsValues(), null, {}, false, {}, onSettingsEvent = events::add,
                cameraControls = { CameraControls(binding) })
        } }
        try {
            compose.onNodeWithText("Start camera").performScrollTo().assertIsNotEnabled()
            compose.onNodeWithText("Apply").performScrollTo().performClick()
            compose.runOnIdle {
                assertEquals(0, opened)
                assertEquals(listOf(SettingsEvent.Stream(20000, 60)), events)
                invitations.value = CameraEndpoint("a".repeat(64), 12345, 1280, 720, 30, 3000, requestedLens = CameraLens.FRONT)
            }
            compose.onNodeWithText("Stop camera").performScrollTo().performClick()
            compose.runOnIdle {
                assertEquals(1, opened); assertEquals(1, closed)
                assertEquals(1, events.size) // Camera stop did not change display/input settings.
                permission = false
            }
            compose.onNodeWithText("Start camera").performScrollTo().performClick()
            compose.runOnIdle { assertEquals(1, requests); binding.permissionResult(false); assertEquals(1, opened); permission = true }
            compose.onNodeWithText("Start camera").performScrollTo().performClick()
            compose.runOnIdle { assertEquals(2, opened) }
            compose.onNodeWithText("Stop camera").performScrollTo().performClick()
            compose.runOnIdle { assertEquals(2, closed); invitations.value = null }
            compose.onNodeWithText("Start camera").assertIsNotEnabled()
        } finally { compose.runOnIdle { binding.shutdown(); scope.cancel() } }
    }

    @Test fun t539_webcamSelectionRequiresHostAndSwitchesOneCamera() {
        val invitations = MutableStateFlow<CameraEndpoint?>(null)
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
        val opened = mutableListOf<CameraLens>()
        val closed = mutableListOf<CameraLens>()
        val binding = CameraBinding(RuntimeEnvironment.getApplication(), { 0 }, {}, { true },
            { _, lens, _, resources -> opened.add(lens); resources.own { closed.add(lens) }; awaitCancellation() }, invitations, scope)
        binding.start()
        compose.setContent { BlentTheme { CameraControls(binding) } }
        compose.onNodeWithText("Front").assertDoesNotExist()
        compose.onNodeWithText("Rear").assertDoesNotExist()
        compose.runOnIdle { invitations.value = CameraEndpoint("a".repeat(64), 12345, 1280, 720, 30, 3000) }
        compose.runOnIdle {
            assertTrue(opened.isEmpty()) // T539 legacy invitations still cannot capture.
            invitations.value = invitations.value!!.copy(requestedLens = CameraLens.FRONT)
        }
        compose.onNodeWithText("Front camera selected.").assertExists()
        compose.runOnIdle { invitations.value = invitations.value!!.copy(requestedLens = CameraLens.REAR) }
        compose.onNodeWithText("Rear camera selected.").assertExists()
        compose.runOnIdle { invitations.value = null }
        compose.onNodeWithText("Off").assertDoesNotExist()
        compose.runOnIdle {
            assertEquals(listOf(CameraLens.FRONT, CameraLens.REAR), opened)
            assertEquals(opened, closed)
            binding.stop(); scope.cancel()
        }
    }
}
