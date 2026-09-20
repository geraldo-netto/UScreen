package com.uscreen

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import androidx.compose.runtime.*
import kotlinx.coroutines.*
import kotlinx.coroutines.android.asCoroutineDispatcher
import kotlinx.coroutines.flow.StateFlow

/** UI/permission owner. Permission or host invitation alone never starts a lens. */
internal class CameraBinding(
    context: Context,
    private val rotation: () -> Int,
    private val requestPermission: () -> Unit,
    private val permission: () -> Boolean = { context.checkSelfPermission(Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED },
    private val capture: suspend (CameraEndpoint, CameraLens, Int, CameraResources) -> Unit = CameraCapture(context)::stream,
    private val invitations: StateFlow<CameraEndpoint?> = CameraInvitations.endpoint,
    private val scope: CoroutineScope = CoroutineScope(SupervisorJob() + android.os.Handler(android.os.Looper.getMainLooper()).asCoroutineDispatcher()),
) {
    var endpoint by mutableStateOf<CameraEndpoint?>(null); private set
    var selected by mutableStateOf<CameraLens?>(null); private set
    var status by mutableStateOf("Start camera sharing on your computer."); private set
    private var active = false
    private var pending: Pair<CameraEndpoint, CameraLens>? = null
    private var observer: Job? = null
    private var worker: Job? = null
    private var resources: CameraResources? = null
    private var generation = 0L

    fun start() {
        if (active) return
        active = true
        observer = scope.launch {
            invitations.collect { value ->
                if (endpoint != value) {
                    stopCapture()
                    endpoint = value
                    pending = null
                    status = if (value == null) "Start camera sharing on your computer." else "Camera sharing is off."
                }
            }
        }
    }

    fun choose(lens: CameraLens?) {
        val previous = stopCapture()
        pending = null
        status = "Camera sharing is off."
        if (!active || lens == null) return
        val host = endpoint ?: return
        if (!permission()) {
            pending = host to lens
            requestPermission()
            return
        }
        launch(host, lens, previous)
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
                if (generation == revision) selected = null
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

    fun stop() {
        active = false
        observer?.cancel()
        observer = null
        pending = null
        stopCapture()
        endpoint = null
        status = "Camera sharing is off."
    }
}
