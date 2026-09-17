package com.uscreen

import android.content.Context
import android.content.pm.ActivityInfo
import android.hardware.Sensor
import android.hardware.SensorManager
import android.view.OrientationEventListener
import android.view.Surface
import android.view.WindowManager
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.shadows.ShadowSensor
import org.robolectric.util.ReflectionHelpers

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34])
class OrientationTest {
    @Test fun t238_naturallyLandscapePanelsFollowBothFlips() {
        for (rotation in rotations) withListener(true, rotation) { activity, listener ->
            listener.onOrientationChanged(0)
            assertEquals("T238: natural landscape at rotation $rotation",
                ActivityInfo.SCREEN_ORIENTATION_LANDSCAPE, activity.requestedOrientation)
            assertStable(activity, listener, listOf(-1, 36, 90, 144))
            listener.onOrientationChanged(180)
            assertEquals(ActivityInfo.SCREEN_ORIENTATION_REVERSE_LANDSCAPE, activity.requestedOrientation)
            assertStable(activity, listener, listOf(-1, 216, 270, 324))
            listener.onOrientationChanged(359)
            assertEquals(ActivityInfo.SCREEN_ORIENTATION_LANDSCAPE, activity.requestedOrientation)
        }
    }

    @Test fun t238_naturallyPortraitPanelsRetainLandscapeMapping() {
        for (rotation in rotations) withListener(false, rotation) { activity, listener ->
            listener.onOrientationChanged(270)
            assertEquals(ActivityInfo.SCREEN_ORIENTATION_LANDSCAPE, activity.requestedOrientation)
            assertStable(activity, listener, listOf(-1, 0, 54, 234, 306))
            listener.onOrientationChanged(90)
            assertEquals(ActivityInfo.SCREEN_ORIENTATION_REVERSE_LANDSCAPE, activity.requestedOrientation)
            assertStable(activity, listener, listOf(-1, 126, 180))
        }
    }

    private val rotations = listOf(Surface.ROTATION_0, Surface.ROTATION_90,
        Surface.ROTATION_180, Surface.ROTATION_270)

    private fun assertStable(activity: MainActivity, listener: OrientationEventListener, angles: List<Int>) {
        val before = activity.requestedOrientation
        angles.forEach { angle ->
            listener.onOrientationChanged(angle)
            assertEquals("T238: unknown/portrait/dead-band angle $angle", before, activity.requestedOrientation)
        }
    }

    @Suppress("DEPRECATION")
    private fun withListener(naturalLandscape: Boolean, rotation: Int,
                             check: (MainActivity, OrientationEventListener) -> Unit) {
        val app = RuntimeEnvironment.getApplication()
        Prefs(app).apply { orientation = Prefs.ORIENTATION_AUTO; checkUpdates = false }
        val sensor = ShadowSensor.newInstance(Sensor.TYPE_ACCELEROMETER)
        shadowOf(app.getSystemService(Context.SENSOR_SERVICE) as SensorManager).addSensor(sensor)
        val display = (app.getSystemService(Context.WINDOW_SERVICE) as WindowManager).defaultDisplay
        val quarterTurn = rotation == Surface.ROTATION_90 || rotation == Surface.ROTATION_270
        val wide = naturalLandscape != quarterTurn
        shadowOf(display).apply {
            setRotation(rotation)
            setRealWidth(if (wide) 2560 else 1600)
            setRealHeight(if (wide) 1600 else 2560)
        }
        val controller = Robolectric.buildActivity(MainActivity::class.java).create().start()
        try {
            val listener = ReflectionHelpers.getField<OrientationEventListener>(controller.get(), "tiltListener")
            assertNotNull("T238: production orientation listener", listener)
            check(controller.get(), listener)
        } finally { controller.stop().destroy() }
    }
}
