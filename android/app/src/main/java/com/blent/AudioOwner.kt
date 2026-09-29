package com.blent

import android.content.Context
import android.content.Intent

internal object AudioOwner {
    private var microphone: AudioBinding? = null
    var permissionRequest: (() -> Unit)? = null
    fun get(context: Context): AudioBinding {
        val app = context.applicationContext
        return microphone ?: AudioBinding(app, { permissionRequest?.invoke() }, service = { run ->
            val intent = Intent(app, MicrophoneService::class.java).putExtra("audio_run", run)
            if (run == null) app.stopService(intent) else app.startForegroundService(intent)
        }).also { microphone = it }
    }
    fun owns(run: String?): Boolean = microphone?.ownsBackground(run) == true
    fun stopped(run: String?) { microphone?.backgroundStopped(run) }
    fun stopSharing() { microphone?.stopSharing() }
    fun reset() { microphone?.shutdown(); microphone = null; permissionRequest = null; AudioInvitations.microphone.value = null }
}
