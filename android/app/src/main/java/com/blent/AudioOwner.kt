package com.blent

import android.content.Context
import android.content.Intent

internal object AudioOwner {
    private var microphone: AudioBinding? = null
    private var speakers: AudioBinding? = null
    var permissionRequest: (() -> Unit)? = null
    private fun cached(direction: Int): AudioBinding? = when (direction) {
        1 -> microphone
        2 -> speakers
        else -> error("Invalid audio direction.")
    }
    fun get(context: Context, direction: Int = 1): AudioBinding {
        cached(direction)?.let { return it }
        val app = context.applicationContext
        val capture = if (direction == 1) AudioCapture(app)::run else AudioPlayback(app)::run
        val service = if (direction == 1) MicrophoneService::class.java else SpeakerService::class.java
        val binding = AudioBinding(app, { permissionRequest?.invoke() },
            permission = { direction == 2 || app.checkSelfPermission(android.Manifest.permission.RECORD_AUDIO) == android.content.pm.PackageManager.PERMISSION_GRANTED },
            capture = capture, invitations = if (direction == 1) AudioInvitations.microphone else AudioInvitations.speakers,
            direction = direction, service = { run ->
            val intent = Intent(app, service).putExtra("audio_run", run)
            if (run == null) app.stopService(intent) else app.startForegroundService(intent)
        })
        if (direction == 1) microphone = binding else speakers = binding
        return binding
    }
    fun owns(run: String?, direction: Int = 1): Boolean = cached(direction)?.ownsBackground(run) == true
    fun stopped(run: String?, direction: Int = 1) { cached(direction)?.backgroundStopped(run) }
    fun stopSharing(direction: Int = 1) { cached(direction)?.stopSharing() }
    fun reset() {
        microphone?.shutdown(); speakers?.shutdown(); microphone = null; speakers = null
        permissionRequest = null; AudioInvitations.microphone.value = null; AudioInvitations.speakers.value = null
    }
}
