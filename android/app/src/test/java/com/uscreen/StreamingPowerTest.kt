package com.uscreen

import android.content.Intent
import android.net.wifi.WifiManager
import android.os.PowerManager
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.Implementation
import org.robolectric.annotation.Implements
import org.robolectric.shadow.api.Shadow

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34], shadows = [PowerTestWifiManager::class, PowerTestWifiLock::class])
class StreamingPowerTest {
    private fun held(service: StreamingService, name: String): Boolean {
        val value = StreamingService::class.java.getDeclaredField(name).apply { isAccessible = true }.get(service)
        return when (value) {
            is PowerManager.WakeLock -> value.isHeld
            is WifiManager.WifiLock -> value.isHeld
            else -> false
        }
    }

    @Test fun t388_battery_profile_releases_usb_and_waiting_locks() {
        val controller = Robolectric.buildService(StreamingService::class.java).create()
        val service = controller.get()
        try {
            service.onStartCommand(Intent().putExtra("battery_saver", true)
                .putExtra("stream_connected", true).putExtra("stream_transport", "usb"), 0, 1)
            assertFalse("T388: USB battery profile must not hold the Wi-Fi radio", held(service, "wifiLock"))
            assertFalse("T388: visible Activity already keeps the screen/device awake", held(service, "wakeLock"))
        } finally { controller.destroy() }
    }

    private fun update(service: StreamingService, active: Boolean, route: String, saver: Boolean = true) {
        service.onStartCommand(Intent().putExtra("battery_saver", saver)
            .putExtra("stream_connected", active).putExtra("stream_transport", route), 0, 2)
    }

    @Test fun t388_network_disconnect_hysteresis_cancels_on_recovery_and_destroy() {
        val controller = Robolectric.buildService(StreamingService::class.java).create()
        val service = controller.get()
        val looper = org.robolectric.Shadows.shadowOf(android.os.Looper.getMainLooper())
        try {
            update(service, true, "network")
            assertTrue(held(service, "wifiLock")); assertFalse(held(service, "wakeLock"))
            update(service, false, "unknown")
            looper.idleFor(java.time.Duration.ofSeconds(4))
            assertTrue(held(service, "wifiLock"))
            update(service, false, "unknown") // Repeated updates must not extend the deadline.
            looper.idleFor(java.time.Duration.ofSeconds(1))
            assertFalse(held(service, "wifiLock"))
            update(service, true, "unknown") // Older hosts get conservative network behavior.
            assertTrue(held(service, "wifiLock"))
            update(service, false, "unknown")
            looper.idleFor(java.time.Duration.ofSeconds(4))
            update(service, true, "network")
            looper.idleFor(java.time.Duration.ofSeconds(2))
            assertTrue(held(service, "wifiLock"))
            update(service, true, "usb")
            assertFalse(held(service, "wifiLock"))
            update(service, true, "network")
            update(service, false, "unknown")
        } finally { controller.destroy() }
        looper.idleFor(java.time.Duration.ofSeconds(6))
        assertFalse(held(service, "wifiLock")); assertFalse(held(service, "wakeLock"))
    }

    @Test fun t388_foreground_rejection_releases_previously_acquired_locks() {
        val controller = Robolectric.buildService(StreamingService::class.java).create()
        val service = controller.get()
        try {
            update(service, true, "network", false)
            org.robolectric.Shadows.shadowOf(service).setThrowInStartForeground(SecurityException("T388 rejected"))
            update(service, true, "network", false)
            assertFalse(held(service, "wifiLock")); assertFalse(held(service, "wakeLock"))
            assertTrue(org.robolectric.Shadows.shadowOf(service).isStoppedBySelf)
        } finally { controller.destroy() }
    }

    @Test fun t388_foreground_promotion_does_not_wait_for_start_command() {
        val controller = Robolectric.buildService(StreamingService::class.java).create()
        try { assertNotNull(org.robolectric.Shadows.shadowOf(controller.get()).lastForegroundNotification) }
        finally { controller.destroy() }
    }

    @Test fun t388_normal_mode_retains_existing_locks_and_destroy_releases_them() {
        val controller = Robolectric.buildService(StreamingService::class.java).create()
        val service = controller.get()
        service.onStartCommand(Intent(), 0, 1)
        service.onStartCommand(Intent(), 0, 2)
        assertTrue(held(service, "wakeLock")); assertTrue(held(service, "wifiLock"))
        controller.destroy()
        assertFalse(held(service, "wakeLock")); assertFalse(held(service, "wifiLock"))
    }
}

/** T388: isolate lock ownership from Robolectric 4.17's newer BlockingOption API. */
@Implements(WifiManager::class)
class PowerTestWifiManager {
    @Implementation
    fun createWifiLock(mode: Int, tag: String): WifiManager.WifiLock =
        Shadow.newInstanceOf(WifiManager.WifiLock::class.java)
}

@Implements(WifiManager.WifiLock::class)
class PowerTestWifiLock {
    private var held = false
    @Implementation fun setReferenceCounted(enabled: Boolean) = Unit
    @Implementation fun acquire() { held = true }
    @Implementation fun release() { check(held); held = false }
    @Implementation fun isHeld(): Boolean = held
}
