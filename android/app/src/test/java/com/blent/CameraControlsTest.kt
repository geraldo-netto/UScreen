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
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger

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
        val opened = AtomicInteger()
        val closed = AtomicInteger()
        // T723: native retirement is asynchronous; hold it across replacement Start.
        val retire = CountDownLatch(1)
        val events = mutableListOf<SettingsEvent>()
        val binding = CameraBinding(RuntimeEnvironment.getApplication(), { 0 }, { requests++ }, { permission },
            { _, _, _, resources ->
                opened.incrementAndGet()
                resources.own { closed.incrementAndGet(); check(retire.await(5, TimeUnit.SECONDS)) }
                awaitCancellation()
            }, invitations, scope)
        binding.start()
        compose.setContent { BlentTheme {
            SettingsSheet(SettingsValues(), null, {}, false, {}, onSettingsEvent = events::add,
                cameraControls = { CameraControls(binding) })
        } }
        try {
            compose.onNodeWithText("Start camera").performScrollTo().assertIsNotEnabled()
            compose.onNodeWithText("Apply").performScrollTo().performClick()
            compose.runOnIdle {
                assertEquals(0, opened.get())
                assertEquals(listOf(SettingsEvent.Stream(20000, 60)), events)
                invitations.value = CameraEndpoint("a".repeat(64), 12345, 1280, 720, 30, 3000, requestedLens = CameraLens.FRONT)
            }
            compose.onNodeWithText("Stop camera").performScrollTo().performClick()
            compose.waitUntil(5000) { closed.get() == 1 }
            compose.runOnIdle {
                assertEquals(1, opened.get()); assertEquals(1, closed.get())
                assertEquals(1, events.size) // Camera stop did not change display/input settings.
                permission = false
            }
            compose.onNodeWithText("Start camera").performScrollTo().performClick()
            compose.runOnIdle { assertEquals(1, requests); binding.permissionResult(false); assertEquals(1, opened.get()); permission = true }
            compose.onNodeWithText("Start camera").performScrollTo().performClick()
            compose.runOnIdle {
                assertEquals("T723 replacement must wait for native retirement", 1, opened.get())
                retire.countDown()
            }
            compose.waitUntil(5000) { opened.get() == 2 }
            compose.runOnIdle { assertEquals(2, opened.get()) }
            compose.onNodeWithText("Stop camera").performScrollTo().performClick()
            compose.waitUntil(5000) { closed.get() == 2 }
            compose.runOnIdle { assertEquals(2, closed.get()); invitations.value = null }
            compose.onNodeWithText("Start camera").assertIsNotEnabled()
        } finally { retire.countDown(); compose.runOnIdle { binding.shutdown(); scope.cancel() } }
    }

    @Test fun t539_webcamSelectionRequiresHostAndSwitchesOneCamera() {
        val invitations = MutableStateFlow<CameraEndpoint?>(null)
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
        val opened = java.util.Collections.synchronizedList(mutableListOf<CameraLens>())
        val closed = java.util.Collections.synchronizedList(mutableListOf<CameraLens>())
        // T725: Compose idle does not wait for native cleanup on Dispatchers.IO.
        val frontRetired = CountDownLatch(1)
        val rearRetired = CountDownLatch(1)
        val binding = CameraBinding(RuntimeEnvironment.getApplication(), { 0 }, {}, { true },
            { _, lens, _, resources ->
                opened.add(lens)
                resources.own {
                    val gate = if (lens == CameraLens.FRONT) frontRetired else rearRetired
                    check(gate.await(5, TimeUnit.SECONDS)); closed.add(lens)
                }
                awaitCancellation()
            }, invitations, scope)
        try {
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
            compose.waitUntil(5000) { opened.size == 1 }
            compose.runOnIdle { invitations.value = invitations.value!!.copy(requestedLens = CameraLens.REAR) }
            compose.onNodeWithText("Rear camera selected.").assertExists()
            compose.runOnIdle {
                assertEquals(listOf(CameraLens.FRONT), opened)
                assertTrue("T725 close is still pending", closed.isEmpty())
                frontRetired.countDown()
            }
            compose.waitUntil(5000) { opened.size == 2 }
            compose.runOnIdle { invitations.value = null }
            compose.onNodeWithText("Off").assertDoesNotExist()
            compose.runOnIdle {
                assertEquals(listOf(CameraLens.FRONT), closed)
                rearRetired.countDown()
            }
            compose.waitUntil(5000) { closed.size == 2 }
            compose.runOnIdle {
                assertEquals("T725 fixture advanced before native retirement", listOf(CameraLens.FRONT, CameraLens.REAR), opened)
                assertEquals(opened, closed)
                binding.stop(); scope.cancel()
            }
        } finally {
            frontRetired.countDown(); rearRetired.countDown()
            compose.runOnIdle { binding.shutdown(); scope.cancel() }
        }
    }
}
