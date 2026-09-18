package com.uscreen

import android.view.MotionEvent
import android.view.SurfaceView
import okhttp3.WebSocket

/** Activity-facing input facade. The shared monitor orders socket handshakes and input. */
class TouchCapture(factory: WebSocket.Factory = defaultControlClient()) {
    companion object {
        const val TAG = "UScreenTouch"
        const val WS_URL = "ws://127.0.0.1:8891"
        const val RECONNECT_DELAY_MS = 2000L
    }

    internal val motion: MotionTranslator = MotionTranslator { message, sampleTimeMs -> control.sendWhenConnected(message, sampleTimeMs) }
    internal val control: ControlSession = ControlSession(this, motion, factory)
    val connectionGeneration get() = control.connectionGeneration
    val controlConnected get() = control.controlConnected
    internal val connectionState get() = control.connectionState
    val isPenOnly get() = control.isPenOnly
    var token: String?
        get() = control.token
        set(value) { control.token = value }
    var onModeKnown: ((Boolean) -> Unit)?
        get() = control.onModeKnown
        set(value) { control.onModeKnown = value }
    var onCodecKnown: ((String) -> Unit)?
        get() = control.onCodecKnown
        set(value) { control.onCodecKnown = value }
    var onFpsKnown: ((Int) -> Unit)?
        get() = control.onFpsKnown
        set(value) { control.onFpsKnown = value }

    internal var onStreamFormat: ((DecoderFormat?) -> Unit)?
        get() = control.onStreamFormat
        set(value) { control.onStreamFormat = value }

    internal val settingsGeneration get() = control.settingsGeneration
    internal var onSettingsRejected: ((RejectedStreamSettings) -> Unit)?
        get() = control.onSettingsRejected
        set(value) { control.onSettingsRejected = value }

    fun setNativeResolution(width: Int, height: Int, widthMm: Int = 0, heightMm: Int = 0) =
        control.setNativeResolution(width, height, widthMm, heightMm)
    fun connect() = control.connect()
    fun disconnect() = control.disconnect()
    internal fun controlStatistics() = control.statistics()
    fun isControlConnected() = control.isControlConnected()
    fun sendConfig(bitrateKbps: Int, fps: Int) = control.sendConfig(bitrateKbps, fps)
    fun sendMode(penOnly: Boolean) = control.sendMode(penOnly)
    fun sendRendered(seq: Int, decodeUs: Int, decoder: String? = null) = control.sendRendered(seq, decodeUs, decoder)

    // The surface only forwards touches to the host; there is no click to perform.
    @android.annotation.SuppressLint("ClickableViewAccessibility")
    fun setSurfaceView(surface: SurfaceView) {
        surface.setOnTouchListener { view, event ->
            handleMotionEvent(event, view.width, view.height)
            true
        }
        surface.setOnHoverListener { view, event ->
            isControlConnected() && motion.handleSurfaceHover(event, view.width, view.height)
        }
    }

    fun handleHoverEvent(event: MotionEvent, width: Int, height: Int): Boolean =
        isControlConnected() && motion.handleHoverEvent(event, width, height)

    @Synchronized fun handleMotionEvent(event: MotionEvent, width: Int, height: Int): Boolean =
        isControlConnected() && motion.handleMotionEvent(event, width, height)
}
