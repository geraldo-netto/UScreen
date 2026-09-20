package com.uscreen

import android.content.Intent
import android.os.Bundle
import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import kotlin.math.roundToInt

class MainActivity : ComponentActivity() {
    internal lateinit var cameras: CameraBinding; private set
    private val cameraPermission = registerForActivityResult(androidx.activity.result.contract.ActivityResultContracts.RequestPermission()) {
        cameras.permissionResult(it)
    }
    internal lateinit var session: SessionCoordinator; private set
    private lateinit var powerBinding: StreamingPowerBinding
    internal lateinit var windowPolicy: ActivityWindowPolicy; private set

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        val prefs = Prefs(this)
        cameras = CameraBinding(this, { @Suppress("DEPRECATION") windowManager.defaultDisplay.rotation },
            { cameraPermission.launch(android.Manifest.permission.CAMERA) })
        powerBinding = StreamingPowerBinding(this)
        windowPolicy = ActivityWindowPolicy(this, prefs)
        windowPolicy.applyDisplaySettings()
        windowPolicy.applyOrientation()
        session = SessionCoordinator(prefs, { runOnUiThread(it) })
        reportNativeResolution()
        setContent {
            UScreenTheme {
                UScreenMain(
                    penOnly = session.penOnlyMode,
                    updateAvailable = session.updateAvailable,
                    showThanks = session.showThanks,
                    onDismissThanks = session::dismissThanks,
                    presentation = session.presentation,
                    settings = session.settings,
                    onSurfaceReady = session::surfaceReady,
                    onSurfaceDestroyed = session::surfaceDestroyed,
                    displayRefreshRates = windowPolicy.supportedDisplayModes().map { it.refreshRate },
                    onSettingsEvent = ::settingsEvent,
                    cameraControls = { CameraControls(cameras) },
                )
            }
        }
        window.decorView.post { windowPolicy.enableImmersiveMode() }
    }

    private fun settingsEvent(event: SettingsEvent) {
        session.handle(event)
        when (event) {
            is SettingsEvent.Orientation -> windowPolicy.applyOrientation()
            is SettingsEvent.Brightness, is SettingsEvent.RefreshRate -> windowPolicy.applyDisplaySettings()
            else -> Unit
        }
    }

    private fun reportNativeResolution() {
        // Report the real screen size (landscape-oriented) so the host can
        // size the virtual display to match this tablet exactly.
        @Suppress("DEPRECATION")
        val size = android.graphics.Point().also {
            windowManager.defaultDisplay.getRealSize(it)
        }
        if (size.x > 0 && size.y > 0) {
            val w = maxOf(size.x, size.y)
            val h = minOf(size.x, size.y)
            // Physical size too: the host puts it in the EDID, and the desktop
            // derives its DPI — and therefore its default scale — from that.
            // Paired long-edge-to-long-edge so it matches the w/h above
            // regardless of the panel's natural orientation.
            val dm = resources.displayMetrics
            val mmA = if (dm.xdpi > 1f) size.x / dm.xdpi * 25.4f else 0f
            val mmB = if (dm.ydpi > 1f) size.y / dm.ydpi * 25.4f else 0f
            val wMm = maxOf(mmA, mmB).roundToInt()
            val hMm = minOf(mmA, mmB).roundToInt()
            session.nativeResolution(w, h, wMm, hMm)
        }

    }

    override fun onNewIntent(intent: Intent?) {
        super.onNewIntent(intent)
        session.applyToken(restart = true)
    }

    override fun onGenericMotionEvent(event: android.view.MotionEvent): Boolean =
        session.hover(event, window.decorView.width, window.decorView.height) || super.onGenericMotionEvent(event)

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (hasFocus) {
            windowPolicy.enableImmersiveMode()
            windowPolicy.applyDisplaySettings()
        }
    }

    override fun onStart() {
        super.onStart()
        cameras.start()
        windowPolicy.start()
        powerBinding.start(session.powerNow(), session.powerUpdates())
        session.checkUpdate {
            try { packageManager.getPackageInfo(packageName, 0).versionName ?: "0" } catch (_: Exception) { "0" }
        }
        session.start()
    }

    override fun onStop() {
        cameras.stop()
        super.onStop()
        windowPolicy.stop()
        powerBinding.stop()
        session.stop()
    }

    override fun onDestroy() {
        if (::powerBinding.isInitialized) powerBinding.stop()
        super.onDestroy()
        windowPolicy.stop()
    }
}
