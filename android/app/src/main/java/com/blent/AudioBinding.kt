package com.blent

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import androidx.compose.runtime.*
import kotlinx.coroutines.*
import kotlinx.coroutines.android.asCoroutineDispatcher
import kotlinx.coroutines.flow.MutableStateFlow
import java.net.Socket
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicReference

internal class AudioResources {
    private val stopped = AtomicBoolean(false)
    private val socket = AtomicReference<Socket?>(null)
    fun attach(value: Socket?) {
        socket.set(value)
        if (stopped.get()) runCatching { socket.getAndSet(null)?.close() }
    }
    fun cancel() { stopped.set(true); runCatching { socket.getAndSet(null)?.close() } }
}

/** Main-thread consent/settings owner. Native retirement runs on its IO worker. */
internal class AudioBinding(private val context: Context,
    private val requestPermission: () -> Unit,
    private val permission: () -> Boolean = { context.checkSelfPermission(Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED },
    private val capture: suspend (AudioEndpoint, AudioPreferences, (Socket?) -> Unit, (String) -> Unit) -> Unit = AudioCapture(context)::run,
    private val invitations: MutableStateFlow<AudioEndpoint?> = AudioInvitations.microphone,
    private val scope: CoroutineScope = CoroutineScope(SupervisorJob() + android.os.Handler(android.os.Looper.getMainLooper()).asCoroutineDispatcher()),
    private val service: (String?) -> Unit = {},
    val direction: Int = 1,
) {
    val label = if (direction == 1) "microphone" else "speakers"
    var preferences by mutableStateOf(AudioPreferences.load(context, direction)); private set
    var status by mutableStateOf("Start $label on your computer."); private set
    var sharing by mutableStateOf(false); private set
    var pending by mutableStateOf<AudioEndpoint?>(null); private set
    private var visible = false
    private var generation = 0L
    private var observer: Job? = null
    private var worker: Job? = null
    private var resources: AudioResources? = null
    private var backgroundRun: String? = null

    fun start() {
        visible = true
        if (observer?.isActive == true) return
        observer = scope.launch {
            invitations.collect { endpoint ->
                if (endpoint != null) { invitations.value = null; invite(endpoint) }
            }
        }
    }
    private fun invite(endpoint: AudioEndpoint) {
        stopSharing()
        if (!visible) return
        if (!endpoint.valid() || endpoint.direction != direction) { status = "Invalid audio request."; return }
        pending = endpoint
        if (!permission()) { requestPermission(); return }
        accept()
    }
    fun permissionResult(granted: Boolean) {
        if (!visible || pending == null) return
        if (!granted) { pending = null; status = "Microphone permission denied."; return }
        accept()
    }
    fun accept() {
        val requested = pending ?: return
        if (!visible || !permission()) return
        try {
            val endpoint = preferences.effective(requested)
            pending = null
            launch(endpoint)
        } catch (error: Exception) { stopSharing(); status = error.message ?: "Audio unavailable." }
    }
    private fun launch(endpoint: AudioEndpoint) {
        val earlier = worker
        val revision = ++generation
        val owned = AudioResources(); resources = owned
        val settings = preferences
        if (endpoint.background) {
            backgroundRun = java.util.UUID.randomUUID().toString()
            service(backgroundRun)
        }
        sharing = true; status = "Starting $label…"
        worker = scope.launch {
            try {
                earlier?.join()
                capture(endpoint, settings, owned::attach) { text ->
                    scope.launch { if (revision == generation) status = text }
                }
            } catch (error: Exception) {
                if (revision == generation && error !is CancellationException) status = error.message ?: "Audio stopped."
            } finally {
                owned.cancel()
                if (revision == generation) { sharing = false; stopBackground() }
            }
        }
    }
    private fun stopBackground() {
        if (backgroundRun == null) return
        backgroundRun = null; service(null)
    }
    fun stopSharing() {
        generation++
        pending = null
        resources?.cancel(); resources = null
        worker?.cancel()
        sharing = false
        stopBackground()
        status = "${label.replaceFirstChar { it.uppercase() }} sharing is off. Start again on your computer."
    }
    fun configure(value: AudioPreferences) {
        require(value.valid(direction))
        stopSharing(); value.save(context, direction); preferences = value
    }
    fun stop() {
        visible = false
        observer?.cancel(); observer = null
        pending = null
        if (backgroundRun == null) stopSharing()
    }
    fun ownsBackground(run: String?): Boolean = run != null && run == backgroundRun
    fun backgroundStopped(run: String?) { if (ownsBackground(run)) stopSharing() }
    fun shutdown() { stop(); stopSharing(); scope.cancel() }
}
