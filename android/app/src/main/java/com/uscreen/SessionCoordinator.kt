package com.uscreen

import android.os.Handler
import android.os.Looper
import android.util.Log
import androidx.compose.runtime.*
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.distinctUntilChanged

/** Read-only stream observations for Compose; owning a view never starts video. */
internal class StreamPresentation(private val receiver: VideoReceiver?, control: TouchCapture?) {
    var connected by mutableStateOf(false); private set
    var fps by mutableStateOf(0f); private set
    var mbps by mutableStateOf(0f); private set
    val connectionState = control?.connectionState ?: MutableStateFlow(ControlConnection())
    val controlConnected: StateFlow<Boolean> = control?.controlConnected ?: MutableStateFlow(false)
    init {
        val ui = Handler(Looper.getMainLooper())
        // Always enqueue, preserving worker/UI callback ordering (T300/T329).
        receiver?.observeConnection(
            { ui.post { connected = true } }, { ui.post { connected = false } },
        )
    }
    fun sample() { fps = receiver?.getFps() ?: 0f; mbps = receiver?.getMbps() ?: 0f }
}

internal data class SettingsValues(
    val bitrateKbps: Int = Prefs.DEFAULT_BITRATE_KBPS,
    val fps: Int = Prefs.DEFAULT_FPS,
    val brightness: Int = Prefs.DEFAULT_BRIGHTNESS_PERCENT,
    val refreshRate: Float = Prefs.DEFAULT_DISPLAY_REFRESH_RATE,
    val orientation: Int = Prefs.ORIENTATION_AUTO,
    val showStats: Boolean = false,
    val checkUpdates: Boolean = true,
    val batterySaver: Boolean = false,
) {
    companion object {
        fun read(prefs: Prefs) = SettingsValues(prefs.bitrateKbps, prefs.fps, prefs.brightnessPercent,
            prefs.displayRefreshRate, prefs.orientation, prefs.showStats, prefs.checkUpdates, prefs.batterySaver)
    }
}

internal sealed interface SettingsEvent {
    data class Stream(val bitrate: Int, val fps: Int) : SettingsEvent
    data class Mode(val penOnly: Boolean) : SettingsEvent
    data class Orientation(val value: Int) : SettingsEvent
    data class Brightness(val value: Int) : SettingsEvent
    data class RefreshRate(val value: Float) : SettingsEvent
    data class ShowStats(val value: Boolean) : SettingsEvent
    data class BatterySaver(val value: Boolean) : SettingsEvent
    data class CheckUpdates(val value: Boolean) : SettingsEvent
}

/** One owner per Activity instance. Rendering observes state and emits events. */
internal class SessionCoordinator(
    private val prefs: Prefs,
    private val dispatchUi: (() -> Unit) -> Unit,
    private var videoReceiver: VideoReceiver? = VideoReceiver(),
    private var touchCapture: TouchCapture? = TouchCapture(),
) {
    var penOnlyMode by mutableStateOf(false); private set
    var showThanks by mutableStateOf(false); private set
    var updateAvailable by mutableStateOf<String?>(null); private set
    var settings by mutableStateOf(SettingsValues.read(prefs)); private set
    private var updateChecked = false
    private var started = false
    val presentation = StreamPresentation(videoReceiver, touchCapture)

    fun powerNow(): StreamingPower = powerFor(settings.batterySaver, presentation.connected, penOnlyMode, presentation.connectionState.value)
    fun powerUpdates() = combine(
        snapshotFlow { Triple(settings.batterySaver, presentation.connected, penOnlyMode) },
        presentation.connectionState,
    ) { local, connection -> powerFor(local.first, local.second, local.third, connection) }.distinctUntilChanged()

    private fun powerFor(saver: Boolean, video: Boolean, pen: Boolean, connection: ControlConnection) =
        StreamingPower(saver, connection.authenticated && (video || pen), connection.transport)

    init { applyToken(false); connectStreamCallbacks() }

    fun handle(event: SettingsEvent) {
        when (event) {
            is SettingsEvent.Stream -> applyStreamSettings(prefs, touchCapture, videoReceiver, event.bitrate, event.fps)
            is SettingsEvent.Mode -> touchCapture?.sendMode(event.penOnly)
            is SettingsEvent.Orientation -> prefs.orientation = event.value
            is SettingsEvent.Brightness -> prefs.brightnessPercent = event.value
            is SettingsEvent.RefreshRate -> prefs.displayRefreshRate = event.value
            is SettingsEvent.ShowStats -> prefs.showStats = event.value
            is SettingsEvent.BatterySaver -> prefs.batterySaver = event.value
            is SettingsEvent.CheckUpdates -> prefs.checkUpdates = event.value
        }
        settings = SettingsValues.read(prefs)
    }

    fun dismissThanks() { showThanks = false }
    fun surfaceReady(view: android.view.SurfaceView) {
        videoReceiver?.setSurface(view)
        touchCapture?.setSurfaceView(view)
    }
    fun surfaceDestroyed() { videoReceiver?.onSurfaceDestroyed() }
    fun hover(event: android.view.MotionEvent, width: Int, height: Int): Boolean =
        width > 0 && height > 0 && touchCapture?.handleHoverEvent(event, width, height) == true

    fun nativeResolution(width: Int, height: Int, widthMm: Int, heightMm: Int) {
        touchCapture?.setNativeResolution(width, height, widthMm, heightMm)
        videoReceiver?.formatWidth = width
        videoReceiver?.formatHeight = height
    }

    fun start() {
        started = true
        touchCapture?.connect()
        if (prefs.hasUserSettings) touchCapture?.sendConfig(prefs.bitrateKbps, prefs.fps)
    }
    fun stop() {
        started = false
        videoReceiver?.stop()
        touchCapture?.disconnect()
    }
    fun checkUpdate(currentVersion: () -> String) {
        if (updateChecked || !prefs.checkUpdates) return
        updateChecked = true
        val version = currentVersion()
        Thread {
            val found = UpdateCheck.newerThan(version)
            if (found != null) dispatchUi { updateAvailable = found }
        }.start()
    }

    private fun connectStreamCallbacks() {
        // Close the host's latency measurement loop: every acknowledged frame
        // lets the host time encoded-packet readiness to receipt of the
        // render acknowledgement, including the return message path.
        videoReceiver?.onFrameRendered = { seq, decodeUs ->
            touchCapture?.sendRendered(seq, decodeUs)
            // First frame ever on screen: say thanks once, then never again.
            if (!prefs.thankedOnce) {
                prefs.thankedOnce = true
                dispatchUi { showThanks = true }
            }
        }
        videoReceiver?.streamFps = prefs.fps
        touchCapture?.onFpsKnown = { fps ->
            withCurrentControl { videoReceiver?.streamFps = fps }
        }
        touchCapture?.onCodecKnown = { codec ->
            withCurrentControl {
                val mime = if (codec == "hevc") VideoReceiver.MIME_TYPE_HEVC
                           else VideoReceiver.MIME_TYPE
                val vr = videoReceiver
                if (vr != null && vr.mimeType != mime) {
                    Log.i("UScreen", "Host is sending $codec — rebuilding the decoder")
                    if (!penOnlyMode) vr.stop()
                    vr.mimeType = mime
                    if (!penOnlyMode) vr.start()
                }
            }
        }
        touchCapture?.onModeKnown = { penOnly ->
            withCurrentControl {
                penOnlyMode = penOnly
                if (penOnly) videoReceiver?.stop() else videoReceiver?.start()
            }
        }

    }

    // A greeting can already be queued on the UI thread when onStop or a
    // token change retires its socket. Validate again when the action runs.
    private fun withCurrentControl(action: () -> Unit) {
        val source = touchCapture ?: return
        val generation = source.connectionGeneration
        dispatchUi {
            if (started && touchCapture === source && source.connectionGeneration == generation) action()
        }
    }

    fun applyToken(restart: Boolean) {
        val token = prefs.hostToken ?: return
        val changed = touchCapture?.token != token
        touchCapture?.token = token
        videoReceiver?.token = token
        // Active reconnect loops also captured the previous token. Retire them
        // even when control is temporarily disconnected. Background sessions
        // stay stopped; onStart connects with the updated credentials.
        if (started && restart && changed) {
            Log.i("UScreen", "New session token — reconnecting")
            touchCapture?.disconnect()
            touchCapture?.connect()
            if (!penOnlyMode) {
                videoReceiver?.stop()
                videoReceiver?.start()
            }
        }
    }

}

internal fun applyStreamSettings(prefs: Prefs?, touchCapture: TouchCapture?, videoReceiver: VideoReceiver?, bitrateKbps: Int, newFps: Int) {
    prefs?.bitrateKbps = bitrateKbps
    prefs?.fps = newFps
    prefs?.hasUserSettings = true
    videoReceiver?.streamFps = newFps
    touchCapture?.sendConfig(bitrateKbps, newFps)
}

