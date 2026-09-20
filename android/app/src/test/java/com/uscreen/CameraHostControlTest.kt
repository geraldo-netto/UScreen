package com.uscreen

import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Looper
import androidx.test.core.app.ApplicationProvider
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.MutableStateFlow
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34])
class CameraHostControlTest {
    @Test fun t543_hostBackgroundPolicySurvivesActivityStopButRetiresOnHostStop() = runBlocking {
        val commands = MutableStateFlow<CameraEndpoint?>(null)
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
        val events = mutableListOf<String>()
        val services = mutableListOf<Boolean>()
        val binding = CameraBinding(RuntimeEnvironment.getApplication(), { 0 }, {}, { true },
            { _, lens, _, resources -> events.add("open ${lens.label}"); resources.own { events.add("close ${lens.label}") }; awaitCancellation() },
            commands, scope, { services.add(it) })
        binding.start()
        val request = CameraEndpoint("a".repeat(64), 12345, 1280, 720, 30, 3000, CameraLens.FRONT, true)
        commands.value = request
        assertEquals(CameraLens.FRONT, binding.selected)
        binding.stop()
        assertEquals(CameraLens.FRONT, binding.selected)
        binding.start(); binding.stop() // reattachment must not duplicate capture
        commands.value = request.copy(requestedLens = CameraLens.REAR)
        assertEquals(listOf("open Front", "close Front", "open Rear"), events)
        assertEquals(listOf(true), services)
        commands.value = null
        assertNull(binding.selected)
        assertEquals("close Rear", events.last())
        assertEquals(listOf(true, false), services)
        binding.backgroundStopped() // delayed normal service teardown cannot stop a new foreground run
        binding.start()
        commands.value = request.copy(background = false)
        binding.backgroundStopped()
        assertEquals(CameraLens.FRONT, binding.selected)
        binding.stop()
        assertNull(binding.selected)
        binding.start(); commands.value = request
        binding.backgroundStopped() // unexpected service death must release capture
        assertNull(binding.selected)
        assertNull(binding.endpoint)
        binding.shutdown(); scope.cancel()
    }

    @Test fun t543_foregroundPolicyAndPermissionStillGateHostCommands() {
        val commands = MutableStateFlow<CameraEndpoint?>(null)
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
        var allowed = false
        var prompts = 0
        val binding = CameraBinding(RuntimeEnvironment.getApplication(), { 0 }, { prompts++ }, { allowed },
            { _, _, _, _ -> awaitCancellation() }, commands, scope)
        binding.start()
        val request = CameraEndpoint("a".repeat(64), 12345, 1280, 720, 30, 3000, CameraLens.REAR)
        commands.value = request
        assertEquals(1, prompts)
        binding.permissionResult(false)
        assertNull(binding.selected)
        commands.value = request.copy(port = 12346)
        allowed = true
        binding.permissionResult(true)
        assertEquals(CameraLens.REAR, binding.selected)
        binding.stop(); assertNull(binding.selected)
        binding.start()
        commands.value = request.copy(background = true)
        assertEquals(CameraLens.REAR, binding.selected)
        binding.stop(); assertEquals(CameraLens.REAR, binding.selected)
        binding.shutdown(); scope.cancel()
    }

    @Test fun t543_cameraForegroundServiceIsTypedNonStickyAndReleasesWakeLock() {
        val app = RuntimeEnvironment.getApplication()
        CameraOwner.reset()
        val owner = CameraOwner.get(app)
        assertSame(owner, CameraOwner.get(app))
        var requested = false
        CameraOwner.permissionRequest = { requested = true }
        shadowOf(app).denyPermissions(android.Manifest.permission.CAMERA)
        owner.start()
        CameraInvitations.endpoint.value = CameraEndpoint("a".repeat(64), 12345, 1280, 720, 30, 3000, CameraLens.FRONT, true)
        shadowOf(Looper.getMainLooper()).idle()
        assertTrue(requested)
        owner.permissionResult(false)
        val controller = Robolectric.buildService(CameraService::class.java).create()
        val service = controller.get()
        assertNull(service.onBind(null))
        assertEquals(android.app.Service.START_NOT_STICKY, service.onStartCommand(Intent(), 0, 1))
        if (android.os.Build.VERSION.SDK_INT >= 30) assertEquals(ServiceInfo.FOREGROUND_SERVICE_TYPE_CAMERA, service.foregroundServiceType)
        val wake = org.robolectric.shadows.ShadowPowerManager.getLatestWakeLock()
        assertTrue(wake.isHeld)
        service.onStartCommand(null, 0, 2)
        assertTrue(shadowOf(service).isStoppedBySelf)
        controller.destroy()
        assertFalse(wake.isHeld)
        CameraOwner.reset()
    }

    @Test fun t543_nativeOwnerStartsAndStopsOnlyCameraService() {
        val app = RuntimeEnvironment.getApplication()
        CameraOwner.reset()
        shadowOf(app).grantPermissions(android.Manifest.permission.CAMERA)
        val owner = CameraOwner.get(app)
        owner.start()
        CameraInvitations.endpoint.value = CameraEndpoint("c".repeat(64), 12345, 1280, 720, 30, 3000, CameraLens.FRONT, true)
        shadowOf(Looper.getMainLooper()).idle()
        val started = shadowOf(app).nextStartedService
        assertEquals(CameraService::class.java.name, started?.component?.className)
        for (attempt in 0..100) {
            shadowOf(Looper.getMainLooper()).idle()
            if (owner.status == "Front camera unavailable") break
            Thread.sleep(10)
        }
        assertEquals("Front camera unavailable", owner.status)
        assertNull(owner.selected)
        assertEquals(CameraService::class.java.name, shadowOf(app).nextStoppedService?.component?.className)
        CameraOwner.reset()
    }

    @Test fun t543_backgroundPromotionFailureNeverStartsCamera() {
        val commands = MutableStateFlow<CameraEndpoint?>(null)
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
        var captures = 0
        val binding = CameraBinding(RuntimeEnvironment.getApplication(), { 0 }, {}, { true },
            { _, _, _, _ -> captures++ }, commands, scope, { enabled -> if (enabled) error("Background permission unavailable") })
        binding.start()
        commands.value = CameraEndpoint("a".repeat(64), 12345, 1280, 720, 30, 3000, CameraLens.FRONT, true)
        assertEquals(0, captures)
        assertEquals("Background permission unavailable", binding.status)
        assertNull(binding.selected)
        binding.shutdown(); scope.cancel()
    }

    @Test fun t543_hiddenPermissionLossRetiresBackgroundServiceWithoutPrompting() {
        val commands = MutableStateFlow<CameraEndpoint?>(null)
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
        val services = mutableListOf<Boolean>()
        var allowed = true
        var prompts = 0
        val binding = CameraBinding(RuntimeEnvironment.getApplication(), { 0 }, { prompts++ }, { allowed },
            { _, _, _, _ -> awaitCancellation() }, commands, scope, { services.add(it) })
        binding.start()
        val request = CameraEndpoint("a".repeat(64), 12345, 1280, 720, 30, 3000, CameraLens.FRONT, true)
        commands.value = request
        binding.stop()
        allowed = false
        commands.value = request.copy(port = 12346)
        assertEquals("T543 permission loss left background camera service running", listOf(true, false), services)
        assertEquals(0, prompts)
        assertNull(binding.selected)
        assertEquals("Open UScreen on the tablet to allow camera access.", binding.status)
        binding.shutdown(); scope.cancel()
    }

    @Test fun t543_receiverValidatesHostLensAndBackgroundChoice() {
        val app = RuntimeEnvironment.getApplication()
        val base = Intent(app, CameraReceiver::class.java).putExtra("token", "b".repeat(64))
            .putExtra("port", 12345).putExtra("width", 1280).putExtra("height", 720).putExtra("fps", 30).putExtra("bitrate", 3000)
        for (lens in listOf(Int.MIN_VALUE, -2, -1, 0, 1, 2, Int.MAX_VALUE)) {
            val read = CameraEndpoint.read(Intent(base).putExtra("lens", lens).putExtra("background", true))
            assertEquals(lens in -1..1, read != null)
            if (read != null) {
                assertTrue(read.background)
                assertEquals(CameraLens.values().firstOrNull { it.wire == lens }, read.requestedLens)
            }
        }
    }
}
