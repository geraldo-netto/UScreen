package com.blent

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import androidx.compose.runtime.*
import kotlinx.coroutines.*
import kotlinx.coroutines.android.asCoroutineDispatcher
import kotlinx.coroutines.flow.StateFlow

/** Host-command/permission owner. A camera foreground service may extend its lifetime. */
internal class CameraBinding(
    context: Context,
    private val rotation: () -> Int,
    private val requestPermission: () -> Unit,
    private val permission: () -> Boolean = { context.checkSelfPermission(Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED },
    private val capture: suspend (CameraEndpoint, CameraLens, Int, CameraResources) -> Unit = CameraCapture(context)::run,
    private val invitations: StateFlow<CameraEndpoint?> = CameraInvitations.endpoint,
    private val scope: CoroutineScope = CoroutineScope(SupervisorJob() + android.os.Handler(android.os.Looper.getMainLooper()).asCoroutineDispatcher()),
    private val backgroundService: (Boolean) -> Unit = {},
) {
    var endpoint by mutableStateOf<CameraEndpoint?>(null); private set
    var selected by mutableStateOf<CameraLens?>(null); private set
    var status by mutableStateOf("Start camera sharing on your computer."); private set
    private var active = false
    private var backgroundRunning = false
    private var pending: Pair<CameraEndpoint, CameraLens>? = null
    private var observer: Job? = null
    private var worker: Job? = null
    private var resources: CameraResources? = null
    private var generation = 0L

    fun start() {
        if (active) return
        active = true
        if (observer?.isActive == true) return
        observer = scope.launch {
            invitations.collect { value ->
                if (endpoint != value) {
                    stopCapture()
                    endpoint = value
                    pending = null
                    status = if (value == null) "Start camera sharing on your computer." else "Camera sharing is off."
                    choose(value?.requestedLens)
                }
            }
        }
    }

    fun choose(lens: CameraLens?) {
        val previous = stopCapture()
        pending = null
        status = "Camera sharing is off."
        if (lens == null) { background(false); return }
        if (!active && !backgroundRunning) return
        val host = endpoint ?: return
        if (!active && !host.background) { background(false); return }
        if (!allowCapture(host, lens)) return
        try {
            background(host.background)
            launch(host, lens, previous)
        } catch (error: Exception) {
            status = error.message ?: "Camera background service unavailable."
            background(false)
        }
    }

    private fun allowCapture(host: CameraEndpoint, lens: CameraLens): Boolean {
        if (permission()) return true
        background(false)
        if (!active) { status = "Open Blent on the tablet to allow camera access."; return false }
        pending = host to lens
        requestPermission()
        return false
    }

    fun permissionResult(granted: Boolean) {
        val request = pending
        pending = null
        if (!active || request == null) return
        if (!granted) { status = "Camera permission denied."; return }
        if (request.first != endpoint) { status = "Computer connection changed; select camera again."; return }
        choose(request.second)
    }

    private fun launch(host: CameraEndpoint, lens: CameraLens, previous: Job?) {
        val revision = generation
        val owned = CameraResources()
        resources = owned
        selected = lens
        status = "${lens.label} camera selected."
        worker = scope.launch {
            try {
                previous?.join()
                capture(host, lens, rotation(), owned)
            } catch (error: Exception) {
                if (generation == revision && error !is CancellationException) status = error.message ?: "Camera sharing failed."
            } finally {
                owned.close()
                if (generation == revision) { selected = null; background(false) }
            }
        }
    }

    private fun stopCapture(): Job? {
        generation++
        val previous = worker
        previous?.cancel()
        resources?.close()
        resources = null
        selected = null
        return previous
    }

    private fun background(enabled: Boolean) {
        if (backgroundRunning == enabled) return
        backgroundRunning = enabled
        backgroundService(enabled)
    }

    fun stop() {
        active = false
        pending = null
        if (backgroundRunning && selected != null) return
        shutdown()
    }

    fun backgroundStopped() {
        if (backgroundRunning) shutdown()
    }

    fun shutdown() {
        active = false
        observer?.cancel()
        observer = null
        pending = null
        stopCapture()
        endpoint = null
        status = "Camera sharing is off."
        background(false)
    }
}
