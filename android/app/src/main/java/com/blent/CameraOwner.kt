package com.blent

import android.content.Context
import android.content.Intent
import android.hardware.display.DisplayManager
import android.view.Display

/** Process camera owner holds only the application; Activity callbacks detach onStop. */
internal object CameraOwner {
    private var binding: CameraBinding? = null
    var permissionRequest: (() -> Unit)? = null

    fun get(context: Context): CameraBinding {
        val app = context.applicationContext
        return binding ?: CameraBinding(app, {
            app.getSystemService(DisplayManager::class.java).getDisplay(Display.DEFAULT_DISPLAY)?.rotation ?: 0
        }, { permissionRequest?.invoke() }, backgroundService = { enabled ->
            val intent = Intent(app, CameraService::class.java)
            if (enabled) app.startForegroundService(intent) else app.stopService(intent)
        }).also { binding = it }
    }

    fun serviceStopped() {
        binding?.backgroundStopped()
    }

    fun reset() {
        binding?.shutdown()
        CameraInvitations.endpoint.value = null
        binding = null
        permissionRequest = null
    }
}
