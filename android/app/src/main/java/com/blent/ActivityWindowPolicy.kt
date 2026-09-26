package com.blent

import android.app.Activity
import android.os.Build
import android.view.View
import android.view.WindowInsetsController
import kotlin.math.abs

/** Activity-window overrides and sensor ownership; never writes system settings. */
internal class ActivityWindowPolicy(private val activity: Activity, private val prefs: Prefs) {
    fun start() { started = true; applyOrientation() }
    fun stop() { started = false; stopTiltListener() }

    /** Window overrides apply only while Blent is foreground. */
    fun applyDisplaySettings() {
        activity.window.attributes = activity.window.attributes.apply { screenBrightness = prefs.brightnessPercent / 100f }
        requestRefreshRate(prefs.displayRefreshRate)
    }

    fun supportedDisplayModes(): List<android.view.Display.Mode> {
        // This WindowManager belongs to the activity's display, including before attachment.
        @Suppress("DEPRECATION")
        val disp = activity.windowManager.defaultDisplay
        val current = disp.mode
        return disp.supportedModes.filter {
            it.physicalWidth == current.physicalWidth && it.physicalHeight == current.physicalHeight
        }
    }

    /** Prefer the requested rate at the current resolution; Android has the final say. */
    fun requestRefreshRate(refreshRate: Float) {
        try {
            val best = if (refreshRate == 0f) null else
                supportedDisplayModes().minByOrNull { abs(it.refreshRate - refreshRate) }

            // Reassigning the same LayoutParams instance can be ignored, so
            // apply the change through an explicit set.
            val lp = activity.window.attributes
            lp.preferredDisplayModeId = best?.modeId ?: 0
            lp.preferredRefreshRate = best?.refreshRate ?: 0f
            activity.window.attributes = lp

            android.util.Log.i(
                "Blent",
                "Requested display mode ${lp.preferredDisplayModeId} @ ${lp.preferredRefreshRate}Hz"
            )
        } catch (e: Exception) {
            android.util.Log.w("Blent", "Could not request a refresh rate: ${e.message}")
        }
    }

    /**
     * Which way round the tablet is held.
     *
     * The manifest's sensorLandscape ought to flip between the two landscape
     * directions on its own, but on the reference tablet (Tab S9 Ultra, One
     * UI, auto-rotate on) it never left "camera up". So the automatic mode
     * reads the tilt sensor itself and pins the direction explicitly, which
     * the system does honour; the manual modes pin it and ignore the sensor.
     */
    private var tiltListener: android.view.OrientationEventListener? = null
    private var started = false

    fun applyOrientation() {
        when (prefs.orientation) {
            Prefs.ORIENTATION_CAMERA_UP -> {
                stopTiltListener()
                activity.requestedOrientation = android.content.pm.ActivityInfo.SCREEN_ORIENTATION_LANDSCAPE
            }
            Prefs.ORIENTATION_CAMERA_DOWN -> {
                stopTiltListener()
                activity.requestedOrientation = android.content.pm.ActivityInfo.SCREEN_ORIENTATION_REVERSE_LANDSCAPE
            }
            else -> {
                activity.requestedOrientation = android.content.pm.ActivityInfo.SCREEN_ORIENTATION_SENSOR_LANDSCAPE
                if (started) startTiltListener()
            }
        }
    }

    @Suppress("DEPRECATION")
    fun startTiltListener() {
        if (tiltListener != null) return
        val display = activity.windowManager.defaultDisplay
        val size = android.graphics.Point().also { display.getRealSize(it) }
        val naturalLandscape = OrientationPolicy.naturallyLandscape(size.x, size.y, display.rotation)
        tiltListener = object : android.view.OrientationEventListener(activity) {
            override fun onOrientationChanged(orientation: Int) {
                val want = OrientationPolicy.landscapeForSensor(orientation, naturalLandscape) ?: return
                if (activity.requestedOrientation != want) activity.requestedOrientation = want
            }
        }.also { if (it.canDetectOrientation()) it.enable() else tiltListener = null }
    }

    fun stopTiltListener() {
        tiltListener?.disable()
        tiltListener = null
    }

    fun enableImmersiveMode() {
        try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                activity.window.setDecorFitsSystemWindows(false)
                activity.window.insetsController?.let { controller ->
                    controller.hide(
                        android.view.WindowInsets.Type.statusBars() or
                        android.view.WindowInsets.Type.navigationBars()
                    )
                    controller.systemBarsBehavior =
                        WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
                }
            } else {
                @Suppress("DEPRECATION")
                activity.window.decorView.systemUiVisibility = (
                    View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY or
                    View.SYSTEM_UI_FLAG_FULLSCREEN or
                    View.SYSTEM_UI_FLAG_HIDE_NAVIGATION or
                    View.SYSTEM_UI_FLAG_LAYOUT_STABLE or
                    View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION or
                    View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
                )
            }
        } catch (e: Exception) {
            // Fallback: some Samsung firmwares have issues with insetsController
            android.util.Log.w("Blent", "Immersive mode failed: ${e.message}")
        }
    }

}
