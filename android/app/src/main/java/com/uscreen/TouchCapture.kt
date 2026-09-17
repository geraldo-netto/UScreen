package com.uscreen

import android.util.Log
import android.view.MotionEvent
import android.view.SurfaceView
import kotlin.math.atan2
import kotlin.math.cos
import kotlin.math.sin
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import okhttp3.*
import org.json.JSONObject
import java.util.concurrent.TimeUnit

class TouchCapture {
    companion object {
        const val TAG = "UScreenTouch"
        const val WS_URL = "ws://127.0.0.1:8891"
        private const val TOOL_TYPE_PALM = 5
        const val RECONNECT_DELAY_MS = 2000L
    }

    @Volatile private var webSocket: WebSocket? = null
    @Volatile var connectionGeneration = 0L
        private set
    private var connectionWanted = false
    private val touchSlots = mutableMapOf<Int, Int>()
    @Volatile private var isConnected = false
    private val authenticatedControl = MutableStateFlow(false)
    val controlConnected = authenticatedControl.asStateFlow()
    private var reconnectJob: Job? = null
    private var surfaceView: SurfaceView? = null

    /** Set from the host's greeting: it is using us as a graphics tablet for
     *  its own screen, so no video will arrive and none should be waited for. */
    @Volatile var isPenOnly = false
        private set
    @Volatile private var touchEnabled = true
    @Volatile private var penEnabled = true
    var onModeKnown: ((penOnly: Boolean) -> Unit)? = null
    var onCodecKnown: ((codec: String) -> Unit)? = null
    var onFpsKnown: ((fps: Int) -> Unit)? = null

    /// Session token from the host, delivered as an intent extra when the
    /// daemon launches us over adb. Must be the first thing sent on the
    /// socket; without it the host closes the connection unanswered.
    @Volatile var token: String? = null

    /** Settings to (re)send to the host whenever the control channel connects */
    @Volatile private var pendingConfig: JSONObject? = null
    @Volatile private var pendingMode: JSONObject? = null

    /** Tablet's native landscape resolution, reported to the host on connect
     *  so the virtual display can match it automatically. */
    @Volatile private var nativeWidth = 0
    @Volatile private var nativeHeight = 0

    /** Physical panel size in millimetres, so the host can build an EDID that
     *  reports the true DPI and the desktop comes up at a sane scale. */
    @Volatile private var nativeWidthMm = 0
    @Volatile private var nativeHeightMm = 0

    fun setNativeResolution(width: Int, height: Int, widthMm: Int = 0, heightMm: Int = 0) {
        nativeWidth = width
        nativeHeight = height
        nativeWidthMm = widthMm
        nativeHeightMm = heightMm
    }

    private val client = OkHttpClient.Builder()
        .readTimeout(0, TimeUnit.SECONDS)
        .connectTimeout(5, TimeUnit.SECONDS)
        .pingInterval(5, TimeUnit.SECONDS)
        .build()

    private val scope = CoroutineScope(Dispatchers.IO + SupervisorJob())

    private val wsListener = object : WebSocketListener() {
        override fun onOpen(webSocket: WebSocket, response: Response) {
            synchronized(this@TouchCapture) {
                if (isStale(webSocket)) return
                isConnected = true
                Log.i(TAG, "Connected")
                // Authenticate before anything else. If we have no token yet the
                // host will drop us and relaunch the app with one, and the
                // reconnect logic takes it from there.
                token?.let { t ->
                    webSocket.send(JSONObject().apply {
                        put("type", "auth")
                        put("token", t)
                    }.toString())
                } ?: Log.w(TAG, "No session token yet — the host will send one")
                if (nativeWidth > 0 && nativeHeight > 0) {
                    val res = JSONObject().apply {
                        put("type", "resolution")
                        put("width", nativeWidth)
                        put("height", nativeHeight)
                        if (nativeWidthMm > 0 && nativeHeightMm > 0) {
                            put("width_mm", nativeWidthMm)
                            put("height_mm", nativeHeightMm)
                        }
                    }
                    webSocket.send(res.toString())
                    Log.i(TAG, "Reported native resolution: ${nativeWidth}x${nativeHeight} " +
                            "(${nativeWidthMm}x${nativeHeightMm} mm)")
                }
                pendingConfig?.let { webSocket.send(it.toString()) }
                pendingMode?.let {
                    webSocket.send(it.toString())
                    pendingMode = null
                }
            }
        }

        override fun onMessage(webSocket: WebSocket, text: String) {
            synchronized(this@TouchCapture) {
                if (isStale(webSocket)) return
                // The host greets with its mode; everything else it might say is
                // ignored, this channel is otherwise ours to talk on.
                try {
                    val o = JSONObject(text)
                    applyInputGreeting(o)
                    applyDecoderGreeting(o)
                    applyModeGreeting(o)
                    if (o.optString("status") == "connected") authenticatedControl.value = true
                } catch (_: Exception) {}
            }
        }

        override fun onClosing(webSocket: WebSocket, code: Int, reason: String) {
            synchronized(this@TouchCapture) {
                if (isStale(webSocket)) return
                authenticatedControl.value = false
                webSocket.close(1000, null)
            }
        }

        override fun onClosed(webSocket: WebSocket, code: Int, reason: String) {
            synchronized(this@TouchCapture) {
                if (isStale(webSocket)) return
                this@TouchCapture.webSocket = null
                connectionGeneration++
                isConnected = false
                authenticatedControl.value = false
                scheduleReconnect()
            }
        }

        override fun onFailure(webSocket: WebSocket, t: Throwable, response: Response?) {
            synchronized(this@TouchCapture) {
                if (isStale(webSocket)) return
                this@TouchCapture.webSocket = null
                connectionGeneration++
                isConnected = false
                authenticatedControl.value = false
                Log.w(TAG, "Connection failed: ${t.message}")
                scheduleReconnect()
            }
        }

        /**
         * A socket we already replaced or closed on purpose. Its closing
         * callbacks still arrive, and acting on them would undo what we just
         * did: after disconnect() they would schedule a reconnect in the
         * background, and during a token change they would flip isConnected
         * off and cancel the socket that replaced them.
         */
        private fun isStale(ws: WebSocket) = ws !== this@TouchCapture.webSocket
    }

    private fun applyInputGreeting(o: JSONObject) {
        if (o.has("touch")) {
            touchEnabled = o.getBoolean("touch")
            if (!touchEnabled) touchSlots.clear()
        }
        if (o.has("pen")) penEnabled = o.getBoolean("pen")
    }

    private fun applyDecoderGreeting(o: JSONObject) {
        if (o.has("fps")) {
            val fps = o.getInt("fps")
            if (fps in 10..90) onFpsKnown?.invoke(fps)
        }
        if (o.has("codec")) {
            onCodecKnown?.invoke(o.getString("codec"))
        }
    }

    private fun applyModeGreeting(o: JSONObject) {
        if (o.has("pen_only")) {
            val pen = o.getBoolean("pen_only")
            if (pen != isPenOnly) {
                isPenOnly = pen
                Log.i(TAG, "Host mode: ${if (pen) "pen-only" else "display"}")
            }
            onModeKnown?.invoke(pen)
        }
    }

    // The surface only forwards touches to the host; there is no click to perform.
    @android.annotation.SuppressLint("ClickableViewAccessibility")
    fun setSurfaceView(sv: SurfaceView) {
        surfaceView = sv

        sv.setOnTouchListener { view, event ->
            handleMotionEvent(event, view.width, view.height)
            true
        }

        // S-Pen hover: pen near screen moves cursor without clicking.
        // Without this, the first touch always snaps the cursor to the pen
        // position and fires a click simultaneously (jarring).
        sv.setOnHoverListener { view, event ->
            if (!isConnected || !penEnabled) return@setOnHoverListener false
            val vw = view.width.coerceAtLeast(1).toFloat()
            val vh = view.height.coerceAtLeast(1).toFloat()
            when (event.actionMasked) {
                MotionEvent.ACTION_HOVER_ENTER,
                MotionEvent.ACTION_HOVER_MOVE -> {
                    if (isPenLike(event, 0)) sendPenEvent(event, 0, 3, vw, vh)
                }
                MotionEvent.ACTION_HOVER_EXIT -> {
                    if (isPenLike(event, 0)) sendPenProximityExit()
                }
            }
            true
        }
    }

    @Synchronized fun connect() {
        connectionWanted = true
        // Idempotent: a second connect() must not leave the first socket
        // alive with its listener still flipping isConnected. onStart and a
        // token delivered through onNewIntent can both call this.
        if (isConnected) return
        // A reconnect already scheduled by onClosed would open a second
        // socket next to this one; this call supersedes it.
        reconnectJob?.cancel()
        webSocket?.cancel()
        webSocket = null
        connectWebSocket()
    }

    @Synchronized private fun connectWebSocket() {
        connectionGeneration++
        authenticatedControl.value = false
        touchSlots.clear()
        touchEnabled = true
        penEnabled = true
        val previous = webSocket
        webSocket = null
        previous?.cancel()
        val request = Request.Builder()
            .url(WS_URL)
            .build()
        webSocket = client.newWebSocket(request, wsListener)
    }

    @Synchronized private fun scheduleReconnect() {
        reconnectJob?.cancel()
        val generation = connectionGeneration
        reconnectJob = scope.launch {
            delay(RECONNECT_DELAY_MS)
            synchronized(this@TouchCapture) {
                if (connectionWanted && generation == connectionGeneration && !isConnected) {
                    connectWebSocket()
                }
            }
        }
    }

    /**
     * Forward stylus hover so the host's cursor follows the pen before it
     * touches down. Returns true only for pen hover, so nothing else the
     * activity might want to do with generic motion events is disturbed.
     */
    fun handleHoverEvent(event: MotionEvent, width: Int, height: Int): Boolean {
        if (!isConnected || !penEnabled || !isPenLike(event, 0)) return false
        val vw = width.coerceAtLeast(1).toFloat()
        val vh = height.coerceAtLeast(1).toFloat()
        return when (event.actionMasked) {
            MotionEvent.ACTION_HOVER_ENTER,
            MotionEvent.ACTION_HOVER_MOVE -> {
                sendPenEvent(event, 0, 3, vw, vh)
                true
            }
            MotionEvent.ACTION_HOVER_EXIT -> {
                sendPenProximityExit()
                true
            }
            // S-Pen side button. Android delivers BUTTON_PRESS/RELEASE as
            // generic motion, never through the touch listener, so this is
            // the only place they can be caught. Forwarded as the stylus
            // button (right-click in GIMP).
            MotionEvent.ACTION_BUTTON_PRESS -> {
                sendPenButton(true)
                true
            }
            MotionEvent.ACTION_BUTTON_RELEASE -> {
                sendPenButton(false)
                true
            }
            else -> false
        }
    }

    @Synchronized fun handleMotionEvent(event: MotionEvent, width: Int, height: Int): Boolean {
        if (!isConnected || (!touchEnabled && !penEnabled)) return false
        val vw = width.coerceAtLeast(1).toFloat()
        val vh = height.coerceAtLeast(1).toFloat()
        releasePalmContacts(event)
        if (event.actionMasked == MotionEvent.ACTION_DOWN) releaseTouches()
        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN,
            MotionEvent.ACTION_POINTER_DOWN -> sendContact(event, event.actionIndex, 0, vw, vh)
            MotionEvent.ACTION_MOVE -> sendMotionSamples(event, vw, vh)
            MotionEvent.ACTION_UP,
            MotionEvent.ACTION_POINTER_UP -> sendContact(event, event.actionIndex, 1, vw, vh)
            MotionEvent.ACTION_CANCEL -> cancelContacts(event, vw, vh)
        }
        return true
    }

    private fun releasePalmContacts(event: MotionEvent) {
        for (index in 0 until event.pointerCount) {
            if (!isPalm(event, index)) continue
            val slot = touchSlots.remove(event.getPointerId(index)) ?: continue
            sendTouch(0f, 0f, 0.0, 1, slot)
        }
    }

    private fun sendContact(event: MotionEvent, index: Int, action: Int, vw: Float, vh: Float) {
        // Reject a platform palm marker if one reaches the app.
        if (isPalm(event, index)) return
        if (isPenLike(event, index)) {
            sendPenEvent(event, index, action, vw, vh)
        } else {
            sendFinger(event, index, action, vw, vh)
        }
    }

    private fun canForwardPointer(event: MotionEvent, index: Int): Boolean {
        if (isPalm(event, index)) return false
        return if (isPenLike(event, index)) penEnabled else touchEnabled
    }

    private fun sendMotionSamples(event: MotionEvent, vw: Float, vh: Float) {
        for (i in 0 until event.pointerCount) {
            if (!canForwardPointer(event, i)) continue
            if (isPenLike(event, i)) sendPenHistory(event, i, vw, vh)
            sendContact(event, i, 2, vw, vh)
        }
    }

    private fun sendPenHistory(event: MotionEvent, index: Int, vw: Float, vh: Float) {
        // Android batches samples between frames. Forward history too so fast
        // pen strokes retain their shape in drawing applications.
        for (h in 0 until event.historySize) {
            val hx = event.getHistoricalX(index, h) / vw
            val hy = event.getHistoricalY(index, h) / vh
            val hp = event.getHistoricalPressure(index, h).toDouble()
            val (htx, hty) = decomposeTilt(
                getHistoricalAxis(event, MotionEvent.AXIS_TILT, index, h),
                getHistoricalAxis(event, MotionEvent.AXIS_ORIENTATION, index, h))
            emitPen(hx.toDouble(), hy.toDouble(), hp, htx, hty,
                isEraser(event, index), 2)
        }
    }

    private fun cancelContacts(event: MotionEvent, vw: Float, vh: Float) {
        // Release each device independently: a touch release cannot lift a pen.
        for (i in 0 until event.pointerCount) {
            if (!canForwardPointer(event, i)) continue
            if (isPenLike(event, i)) sendPenEvent(event, i, 1, vw, vh)
        }
        releaseTouches()
    }

    // Android pointer IDs can be sparse and exceed the host's 10 slots.
    // Allocate only on DOWN and retain the assignment until UP/CANCEL.
    private fun sendFinger(event: MotionEvent, index: Int, action: Int, vw: Float, vh: Float) {
        if (!touchEnabled) return
        val id = event.getPointerId(index)
        val slot = touchSlots[id] ?: if (action == 0) {
            val free = (0..9).firstOrNull { it !in touchSlots.values } ?: return
            touchSlots[id] = free
            free
        } else return
        sendTouch(event.getX(index) / vw, event.getY(index) / vh,
            if (action == 1) 0.0 else event.getPressure(index).toDouble(), action, slot)
        if (action == 1) touchSlots.remove(id)
    }

    private fun releaseTouches() {
        for (slot in touchSlots.values) sendTouch(0f, 0f, 0.0, 1, slot)
        touchSlots.clear()
    }

    /** Stylus or its eraser end — both drive the pen/tablet device. */
    private fun isPenLike(event: MotionEvent, index: Int): Boolean {
        return try {
            val t = event.getToolType(index)
            t == MotionEvent.TOOL_TYPE_STYLUS || t == MotionEvent.TOOL_TYPE_ERASER
        } catch (_: Exception) {
            false
        }
    }

    private fun isEraser(event: MotionEvent, index: Int): Boolean {
        return try {
            event.getToolType(index) == MotionEvent.TOOL_TYPE_ERASER
        } catch (_: Exception) {
            false
        }
    }

    private fun getAxis(event: MotionEvent, axis: Int, index: Int): Double {
        return try {
            event.getAxisValue(axis, index).toDouble()
        } catch (_: Exception) {
            0.0
        }
    }

    private fun getHistoricalAxis(event: MotionEvent, axis: Int, index: Int, hist: Int): Double {
        return try {
            event.getHistoricalAxisValue(axis, index, hist).toDouble()
        } catch (_: Exception) {
            0.0
        }
    }

    /**
     * Decompose Android's stylus tilt into X/Y tilt angles, **in degrees**.
     *
     * Android exposes AXIS_TILT as the angle from the screen normal (0 =
     * perpendicular, π/2 = flat) and AXIS_ORIENTATION as the azimuth of the
     * tilt around that normal (0..2π). A Wacom-style ABS_TILT_X/Y device wants
     * the signed X and Y tilt *angles*, which are the arctangents of the tilt
     * vector's components projected onto the surface — not the components
     * themselves.
     *
     * The previous version returned the raw projections `sin(tilt)·cos(orient)`
     * (a dimensionless value in [-1,1]) and the host multiplied them by
     * 180/π as if they were radians. A pen laid flat at 90° came out as 57°,
     * and everything in between was wrong non-linearly.
     */
    private fun decomposeTilt(tiltRad: Double, orientationRad: Double): Pair<Double, Double> {
        val sinTilt = sin(tiltRad)
        val cosTilt = cos(tiltRad)
        // Android's AXIS_ORIENTATION is measured clockwise from the top of the
        // screen: 0 points up, +pi/2 points right. So the direction the pen
        // leans, in screen coordinates with y downwards, is
        // (sin(orientation), -cos(orientation)).
        //
        // libinput wants tilt_x positive towards the right edge and tilt_y
        // positive towards the user, which is the bottom edge — hence the
        // minus on the y component. These two were the other way round until
        // 1.2.2, so a pen leaning right reported as leaning down; reported by
        // a Lenovo Tab Pen Plus user in issue #11.
        val tx = atan2(sinTilt * sin(orientationRad), cosTilt)
        val ty = atan2(-sinTilt * cos(orientationRad), cosTilt)
        return Math.toDegrees(tx) to Math.toDegrees(ty)
    }

    private fun sendPenEvent(event: MotionEvent, index: Int, action: Int,
                              vw: Float, vh: Float) {
        if (!penEnabled) return
        val x = event.getX(index) / vw
        val y = event.getY(index) / vh
        val pressure = event.getPressure(index).toDouble()
        val (tiltX, tiltY) = decomposeTilt(
            getAxis(event, MotionEvent.AXIS_TILT, index),
            getAxis(event, MotionEvent.AXIS_ORIENTATION, index))
        emitPen(x.toDouble(), y.toDouble(), pressure, tiltX, tiltY,
            isEraser(event, index), action)
    }

    private fun emitPen(x: Double, y: Double, pressure: Double,
                        tiltX: Double, tiltY: Double, eraser: Boolean, action: Int) {
        if (!penEnabled) return
        val msg = JSONObject().apply {
            put("type", "pen")
            put("x", x.coerceIn(0.0, 1.0))
            put("y", y.coerceIn(0.0, 1.0))
            put("pressure", pressure.coerceIn(0.0, 1.0))
            put("tilt_x", tiltX)
            put("tilt_y", tiltY)
            put("eraser", eraser)
            put("action", action)
        }
        sendWhenConnected(msg)
    }

    private fun sendPenButton(down: Boolean) {
        if (!penEnabled) return
        val msg = JSONObject().apply {
            put("type", "pen")
            put("x", 0.0)
            put("y", 0.0)
            put("pressure", 0.0)
            put("tilt_x", 0.0)
            put("tilt_y", 0.0)
            put("eraser", false)
            // 5 = stylus button down, 6 = stylus button up
            put("action", if (down) 5 else 6)
        }
        sendWhenConnected(msg)
    }

    private fun sendPenProximityExit() {
        if (!penEnabled) return
        val msg = JSONObject().apply {
            put("type", "pen")
            put("x", 0.0)
            put("y", 0.0)
            put("pressure", 0.0)
            put("tilt_x", 0.0)
            put("tilt_y", 0.0)
            put("eraser", false)
            put("action", 4) // HOVER_EXIT / pen left proximity
        }
        sendWhenConnected(msg)
    }

    private fun sendTouch(x: Float, y: Float, pressure: Double,
                          action: Int, slot: Int) {
        if (!touchEnabled) return
        val msg = JSONObject().apply {
            put("type", "touch")
            put("x", x.toDouble().coerceIn(0.0, 1.0))
            put("y", y.toDouble().coerceIn(0.0, 1.0))
            put("pressure", pressure.coerceIn(0.0, 1.0))
            put("action", action)
            put("slot", slot)
        }
        sendWhenConnected(msg)
    }

    /**
     * Push encoder settings to the host. The host live-restarts ffmpeg with
     * the new parameters and persists them in its config file. Settings are
     * also remembered here and re-sent on every reconnect.
     */
    @Synchronized fun sendConfig(bitrateKbps: Int, fps: Int) {
        val msg = JSONObject().apply {
            put("type", "config")
            put("bitrate", bitrateKbps)
            put("fps", fps)
        }
        pendingConfig = msg
        if (isConnected) {
            webSocket?.send(msg.toString())
            Log.i(TAG, "Sent config: $msg")
        }
    }

    /**
     * Tell the host that frame [seq] is on screen. The host started the clock
     * when it emitted that encoded frame. The round trip measures packet send
     * through render acknowledgement, excluding capture and encoding, without
     * either side needing a shared time base.
     */
    fun sendRendered(seq: Int, decodeUs: Int) {
        if (!isConnected) return
        val msg = JSONObject().apply {
            put("type", "rendered")
            // Sent unsigned: the host's counter is a u32 and Kotlin's Int is
            // signed, so it wraps negative after ~2^31 frames (~1 year at
            // 60 fps, but free to get right).
            put("seq", seq.toLong() and 0xFFFFFFFFL)
            // How much of the round trip was spent here (arrival → on screen).
            // The host subtracts it to see what the wire actually costs.
            if (decodeUs >= 0) put("decode_us", decodeUs)
        }
        sendWhenConnected(msg)
    }

    /**
     * Ask the host to switch between being a second screen and being a
     * graphics tablet. The host applies it and answers with its new mode, so
     * the UI follows [onModeKnown] rather than assuming this succeeded.
     */
    @Synchronized fun sendMode(penOnly: Boolean) {
        val msg = JSONObject().apply {
            put("type", "mode")
            put("pen_only", penOnly)
        }
        if (isConnected) {
            webSocket?.send(msg.toString())
            Log.i(TAG, "Requested mode: ${if (penOnly) "pen-only" else "display"}")
        } else {
            // Held rather than replayed forever: the host is the source of
            // truth for the mode, and re-asserting a stale choice on every
            // reconnect would fight whatever it was set to in the meantime.
            pendingMode = msg
        }
    }

    /**
     * AOSP defines the hidden MotionEvent.TOOL_TYPE_PALM as 5. Keep the
     * numeric value because it is outside the public SDK. Android normally
     * filters palms before delivery; this also rejects any that reach us.
     * T303: frameworks/base/core/java/android/view/MotionEvent.java.
     */
    @android.annotation.SuppressLint("WrongConstant")
    private fun isPalm(event: MotionEvent, index: Int): Boolean =
        event.getToolType(index) == TOOL_TYPE_PALM

    fun isControlConnected(): Boolean = isConnected

    // onOpen holds this monitor until auth and initial metadata are queued.
    // UI input and decoder acknowledgements must not overtake that handshake.
    @Synchronized private fun sendWhenConnected(message: JSONObject) {
        if (isConnected) webSocket?.send(message.toString())
    }

    @Synchronized fun disconnect() {
        connectionWanted = false
        touchSlots.clear()
        connectionGeneration++
        reconnectJob?.cancel()
        val previous = webSocket
        webSocket = null
        isConnected = false
        authenticatedControl.value = false
        previous?.close(1000, "Client closing")
    }
}
