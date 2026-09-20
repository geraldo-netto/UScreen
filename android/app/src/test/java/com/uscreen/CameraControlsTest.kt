package com.uscreen

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

    @Test fun t539_webcamSelectionRequiresHostAndSwitchesOneCamera() {
        val invitations = MutableStateFlow<CameraEndpoint?>(null)
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
        val opened = mutableListOf<CameraLens>()
        val closed = mutableListOf<CameraLens>()
        val binding = CameraBinding(RuntimeEnvironment.getApplication(), { 0 }, {}, { true },
            { _, lens, _, resources -> opened.add(lens); resources.own { closed.add(lens) }; awaitCancellation() }, invitations, scope)
        binding.start()
        compose.setContent { UScreenTheme { CameraControls(binding) } }
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
