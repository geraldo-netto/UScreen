// Copyright (c) 2026 Geraldo Netto
package com.blent

import android.content.Intent
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.MutableStateFlow
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.util.ReflectionHelpers

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34])
class CameraServiceOwnershipTest {
    private class Fixture : AutoCloseable {
        val app = RuntimeEnvironment.getApplication()
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
        val invitations = MutableStateFlow<CameraEndpoint?>(null)
        val starts = mutableListOf<Intent>()
        val binding = CameraBinding(app, { 0 }, {}, { true },
            { _, _, _, _ -> awaitCancellation() }, invitations, scope,
            backgroundService = { run -> starts.add(Intent(app, CameraService::class.java).putExtra("camera_run", run)) })
        private val services = mutableListOf<org.robolectric.android.controller.ServiceController<CameraService>>()
        init {
            CameraOwner.reset()
            ReflectionHelpers.setStaticField(CameraOwner::class.java, "binding", binding)
            binding.start()
            invitations.value = CameraEndpoint("a".repeat(64), 12345, 1280, 720, 30, 3000, CameraLens.FRONT, true)
        }
        fun service(): CameraService = Robolectric.buildService(CameraService::class.java).create().also { services.add(it) }.get()
        fun replace() { binding.choose(null); binding.choose(CameraLens.REAR) }
        override fun close() {
            CameraOwner.reset()
            services.forEach { it.destroy() }
            scope.cancel()
        }
    }

    @Test fun t706_destroyingOldServiceCannotStopReplacementBackgroundCapture() = Fixture().use { fixture ->
        val old = fixture.service()
        old.onStartCommand(fixture.starts.last(), 0, 1)
        fixture.replace()
        val replacement = fixture.service()
        replacement.onStartCommand(fixture.starts.last(), 0, 2)
        old.onDestroy()
        assertEquals("T706 old service destroyed the new run", CameraLens.REAR, fixture.binding.selected)
        assertNotNull(fixture.binding.endpoint)
        replacement.onDestroy()
        assertNull("T706 current service death must retire capture", fixture.binding.selected)
        assertNull(fixture.binding.endpoint)
    }

    @Test fun t706_reusedServiceOwnsLatestStartAndIgnoresInvalidDeliveries() = Fixture().use { fixture ->
        val service = fixture.service()
        val old = fixture.starts.last()
        service.onStartCommand(old, 0, 1)
        fixture.replace()
        service.onStartCommand(fixture.starts.last(), 0, 2)
        for (intent in listOf(null, Intent(), old, Intent().putExtra("camera_run", ""), Intent().putExtra("camera_run", "unknown"))) {
            service.onStartCommand(intent, 0, 3)
            assertEquals(CameraLens.REAR, fixture.binding.selected)
            assertFalse(org.robolectric.Shadows.shadowOf(service).isStoppedBySelf)
        }
        service.onDestroy()
        assertNull(fixture.binding.selected)
    }

    @Test fun t706_failedPromotionRetiresOnlyTheDeliveredRun() = Fixture().use { fixture ->
        val service = fixture.service()
        ReflectionHelpers.setField(service, "ready", false)
        val old = fixture.starts.last()
        fixture.replace()
        service.onStartCommand(old, 0, 1)
        assertEquals(CameraLens.REAR, fixture.binding.selected)
        service.onStartCommand(fixture.starts.last(), 0, 2)
        assertNull(fixture.binding.selected)
        assertTrue(org.robolectric.Shadows.shadowOf(service).isStoppedBySelf)
    }

    @Test fun t706_ownerResetCannotReuseServiceIdentity() {
        val old = Fixture().use { it.starts.last().getStringExtra("camera_run") }
        Fixture().use { fixture ->
            assertNotEquals(old, fixture.starts.last().getStringExtra("camera_run"))
            CameraOwner.serviceStopped(old)
            CameraOwner.serviceStopped(null)
            assertEquals(CameraLens.FRONT, fixture.binding.selected)
        }
        assertFalse(CameraOwner.ownsService(old))
        CameraOwner.serviceStopped(old)
    }
}
