package com.blent

import android.hardware.display.DisplayManager
import android.provider.Settings
import android.view.Display
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.shadows.ShadowDisplayManager
import org.robolectric.util.ReflectionHelpers
import org.robolectric.util.ReflectionHelpers.ClassParameter

@RunWith(RobolectricTestRunner::class)
// T539: Activity-created Compose dispatchers must not outlive their test looper.
@Config(sdk = [27, 34], instrumentedPackages = ["androidx.compose.ui.platform"])
class DisplaySettingsTest {
    private val app get() = RuntimeEnvironment.getApplication()

    @Test fun t497_invalidPersistedRefreshValuesFallBackWithoutChangingValidOverrides() {
        val prefs = Prefs(app)
        for (value in listOf(Float.NaN, Float.POSITIVE_INFINITY, Float.NEGATIVE_INFINITY, -1f)) {
            prefs.displayRefreshRate = value
            assertEquals(Prefs.DEFAULT_DISPLAY_REFRESH_RATE, prefs.displayRefreshRate, 0f)
        }
        for (value in listOf(0f, 59.94f, 120f)) {
            prefs.displayRefreshRate = value
            assertEquals(value, prefs.displayRefreshRate, 0f)
        }
    }

    @Test fun t497_activitySettingsEventsApplyWindowAndOrientationPoliciesImmediately() {
        installModes()
        val controller = Robolectric.buildActivity(MainActivity::class.java).create()
        try {
            val activity = controller.get()
            fun change(event: SettingsEvent) = ReflectionHelpers.callInstanceMethod<Unit>(activity, "settingsEvent",
                ClassParameter.from(SettingsEvent::class.java, event))
            change(SettingsEvent.Brightness(70))
            assertEquals(0.7f, activity.window.attributes.screenBrightness, 0f)
            change(SettingsEvent.RefreshRate(0f))
            assertEquals(0, activity.window.attributes.preferredDisplayModeId)
            change(SettingsEvent.Orientation(Prefs.ORIENTATION_CAMERA_DOWN))
            assertEquals(android.content.pm.ActivityInfo.SCREEN_ORIENTATION_REVERSE_LANDSCAPE, activity.requestedOrientation)
            change(SettingsEvent.ShowStats(true))
            assertTrue(activity.session.settings.showStats)
            val hover = android.view.MotionEvent.obtain(0, 0, android.view.MotionEvent.ACTION_HOVER_MOVE, 10f, 10f, 0)
            try { assertFalse(activity.onGenericMotionEvent(hover)) } finally { hover.recycle() }
        } finally { controller.destroy() }
    }

    private fun mode(id: Int, width: Int, height: Int, hz: Float): Display.Mode =
        ReflectionHelpers.callConstructor(Display.Mode::class.java,
            ClassParameter.from(Int::class.javaPrimitiveType, id),
            ClassParameter.from(Int::class.javaPrimitiveType, width),
            ClassParameter.from(Int::class.javaPrimitiveType, height),
            ClassParameter.from(Float::class.javaPrimitiveType, hz))

    private fun installModes(slowHz: Float = 60f) {
        val display = app.getSystemService(DisplayManager::class.java).getDisplay(Display.DEFAULT_DISPLAY)
        val current = display.mode
        ShadowDisplayManager.setSupportedModes(display.displayId,
            mode(current.modeId, current.physicalWidth, current.physicalHeight, 120f),
            mode(101, current.physicalWidth, current.physicalHeight, slowHz),
            mode(102, current.physicalWidth * 2, current.physicalHeight * 2, 60f))
    }

    @Test fun t349_newActivityDefaultsToHalfBrightness() {
        val controller = Robolectric.buildActivity(MainActivity::class.java).create()
        try {
            assertEquals("T349 initial app brightness", 0.5f,
                controller.get().window.attributes.screenBrightness, 0f)
        } finally { controller.destroy() }
    }

    @Test fun t349_defaultRefreshPreservesResolution() {
        installModes()
        val controller = Robolectric.buildActivity(MainActivity::class.java).create()
        try {
            val attributes = controller.get().window.attributes
            assertEquals("T349 selected panel mode: ${org.robolectric.shadows.ShadowLog.getLogsForTag("Blent")}",
                101, attributes.preferredDisplayModeId)
            assertEquals(60f, attributes.preferredRefreshRate, 0f)
        } finally { controller.destroy() }
    }

    @Test fun t349_nearestSupportedRefreshPreservesResolution() {
        installModes(59.94f)
        val controller = Robolectric.buildActivity(MainActivity::class.java).create()
        try {
            val attributes = controller.get().window.attributes
            assertEquals("T349 must not change resolution just to obtain exactly 60 Hz",
                101, attributes.preferredDisplayModeId)
            assertEquals(59.94f, attributes.preferredRefreshRate, 0f)
        } finally { controller.destroy() }
    }

    @Test fun t349_focusRegainReappliesAppDefaultsOnly() {
        installModes()
        val resolver = app.contentResolver
        Settings.System.putInt(resolver, Settings.System.SCREEN_BRIGHTNESS, 210)
        Settings.System.putInt(resolver, Settings.System.SCREEN_BRIGHTNESS_MODE, 1)
        Settings.System.putFloat(resolver, "peak_refresh_rate", 120f)
        val controller = Robolectric.buildActivity(MainActivity::class.java).create()
        try {
            val activity = controller.get()
            activity.window.attributes = activity.window.attributes.apply {
                screenBrightness = 0.9f
                preferredDisplayModeId = 0
                preferredRefreshRate = 120f
            }
            activity.onWindowFocusChanged(false)
            assertEquals(0.9f, activity.window.attributes.screenBrightness, 0f)
            activity.onWindowFocusChanged(true)
            assertEquals(0.5f, activity.window.attributes.screenBrightness, 0f)
            assertEquals(101, activity.window.attributes.preferredDisplayModeId)
            assertEquals(60f, activity.window.attributes.preferredRefreshRate, 0f)
        } finally { controller.destroy() }
        assertEquals(210, Settings.System.getInt(resolver, Settings.System.SCREEN_BRIGHTNESS))
        assertEquals(1, Settings.System.getInt(resolver, Settings.System.SCREEN_BRIGHTNESS_MODE))
        assertEquals(120f, Settings.System.getFloat(resolver, "peak_refresh_rate"), 0f)
        val other = Robolectric.buildActivity(android.app.Activity::class.java).create()
        try {
            assertEquals(-1f, other.get().window.attributes.screenBrightness, 0f)
            assertEquals(0, other.get().window.attributes.preferredDisplayModeId)
            assertEquals(0f, other.get().window.attributes.preferredRefreshRate, 0f)
        } finally { other.destroy() }
    }

    @Test fun t349_savedOverridesSurviveFocusAndRelaunch() {
        installModes()
        app.getSharedPreferences("blent", android.content.Context.MODE_PRIVATE).edit()
            .putInt("brightness_percent", 75).putFloat("display_refresh_rate", 120f).commit()
        repeat(2) {
            val controller = Robolectric.buildActivity(MainActivity::class.java).create()
            try {
                val activity = controller.get()
                assertEquals(0.75f, activity.window.attributes.screenBrightness, 0f)
                assertEquals(120f, activity.window.attributes.preferredRefreshRate, 0f)
                activity.onWindowFocusChanged(true)
                assertEquals(0.75f, activity.window.attributes.screenBrightness, 0f)
                assertEquals(120f, activity.window.attributes.preferredRefreshRate, 0f)
            } finally { controller.destroy() }
        }
    }

    @Test fun t349_systemRefreshOptionClearsAppPreference() {
        installModes()
        val controller = Robolectric.buildActivity(MainActivity::class.java).create()
        try {
            val activity = controller.get()
            app.getSharedPreferences("blent", android.content.Context.MODE_PRIVATE).edit()
                .putFloat("display_refresh_rate", 0f).commit()
            activity.onWindowFocusChanged(true)
            assertEquals(0, activity.window.attributes.preferredDisplayModeId)
            assertEquals(0f, activity.window.attributes.preferredRefreshRate, 0f)
            assertEquals(0.5f, activity.window.attributes.screenBrightness, 0f)
        } finally { controller.destroy() }
    }
}
