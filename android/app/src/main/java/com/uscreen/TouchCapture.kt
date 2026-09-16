package com.uscreen

import android.util.Log
import android.view.MotionEvent
import android.view.SurfaceView
import kotlin.math.atan2
import kotlin.math.cos
import kotlin.math.sin
import kotlinx.coroutines.*
import okhttp3.*
import org.json.JSONObject
import java.util.concurrent.TimeUnit

class TouchCapture {
    companion object {
        const val TAG = "UScreenTouch"
        const val WS_URL = "ws://127.0.0.1:8891"
        private const val TOOL_TYPE_PALM = 6
        const val RECONNECT_DELAY_MS = 2000L
    }

    @Volatile private var webSocket: WebSocket? = null
    @Volatile var connectionGeneration = 0L
        private set
    private var connectionWanted = false
    private val touchSlots = mutableMapOf<Int, Int>()
    @Volatile private var isConnected = false
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
                    if (o.has("touch")) {
                        touchEnabled = o.getBoolean("touch")
                        if (!touchEnabled) touchSlots.clear()
                    }
                    if (o.has("pen")) penEnabled = o.getBoolean("pen")
                    if (o.has("fps")) {
                        val fps = o.getInt("fps")
                        if (fps in 10..90) onFpsKnown?.invoke(fps)
                    }
                    if (o.has("codec")) {
                        onCodecKnown?.invoke(o.getString("codec"))
                    }
                    if (o.has("pen_only")) {
                        val pen = o.getBoolean("pen_only")
                        if (pen != isPenOnly) {
                            isPenOnly = pen
                            Log.i(TAG, "Host mode: ${if (pen) "pen-only" else "display"}")
                        }
                        onModeKnown?.invoke(pen)
                    }
                } catch (_: Exception) {}
            }
        }

        override fun onClosing(webSocket: WebSocket, code: Int, reason: String) {
            synchronized(this@TouchCapture) {
                if (isStale(webSocket)) return
                webSocket.close(1000, null)
            }
        }

        override fun onClosed(webSocket: WebSocket, code: Int, reason: String) {
            synchronized(this@TouchCapture) {
                if (isStale(webSocket)) return
                this@TouchCapture.webSocket = null
                connectionGeneration++
                isConnected = false
                scheduleReconnect()
            }
        }

        override fun onFailure(webSocket: WebSocket, t: Throwable, response: Response?) {
            synchronized(this@TouchCapture) {
                if (isStale(webSocket)) return
                this@TouchCapture.webSocket = null
                connectionGeneration++
                isConnected = false
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
        if (!isConnected || !penEnabled) return false
        val vw = width.coerceAtLeast(1).toFloat()
        val vh = height.coerceAtLeast(1).toFloat()
        return when (event.actionMasked) {
            MotionEvent.ACTION_HOVER_ENTER,
            MotionEvent.ACTION_HOVER_MOVE -> {
                if (!isPenLike(event, 0)) return false
                sendPenEvent(event, 0, 3, vw, vh)
                true
            }
            MotionEvent.ACTION_HOVER_EXIT -> {
                if (!isPenLike(event, 0)) return false
                sendPenProximityExit()
                true
            }
            // S-Pen side button. Android delivers BUTTON_PRESS/RELEASE as
            // generic motion, never through the touch listener, so this is
            // the only place they can be caught. Forwarded as the stylus
            // button (right-click in GIMP).
            MotionEvent.ACTION_BUTTON_PRESS -> {
                if (!isPenLike(event, 0)) return false
                sendPenButton(true)
                true
            }
            MotionEvent.ACTION_BUTTON_RELEASE -> {
                if (!isPenLike(event, 0)) return false
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

        val pointerCount = event.pointerCount
        val actionIndex = event.actionIndex
        val maskedAction = event.actionMasked

        if (maskedAction == MotionEvent.ACTION_DOWN) releaseTouches()
        when (maskedAction) {
            MotionEvent.ACTION_DOWN,
            MotionEvent.ACTION_POINTER_DOWN -> {
                // Drop palm contacts — Samsung sends TOOL_TYPE_PALM for
                // unintentional palm-rest touches; forwarding them causes
                // phantom scrolling on the Linux side.
                if (isPalm(event, actionIndex)) {
                    return true
                }
                if (isPenLike(event, actionIndex)) {
                    sendPenEvent(event, actionIndex, 0, vw, vh)
                } else {
                    sendFinger(event, actionIndex, 0, vw, vh)
                }
            }

            MotionEvent.ACTION_MOVE -> {
                for (i in 0 until pointerCount) {
                    if (isPalm(event, i)) continue
                    if (isPenLike(event, i) && !penEnabled) continue
                    if (!isPenLike(event, i) && !touchEnabled) continue
                    if (isPenLike(event, i)) {
                        // Android batches several samples between frames.
                        // Forward the historical points too, otherwise fast
                        // pen strokes look jagged in GIMP.
                        val hist = event.historySize
                        for (h in 0 until hist) {
                            val hx = event.getHistoricalX(i, h) / vw
                            val hy = event.getHistoricalY(i, h) / vh
                            val hp = event.getHistoricalPressure(i, h).toDouble()
                            val (htx, hty) = decomposeTilt(
                                getHistoricalAxis(event, MotionEvent.AXIS_TILT, i, h),
                                getHistoricalAxis(event, MotionEvent.AXIS_ORIENTATION, i, h))
                            emitPen(hx.toDouble(), hy.toDouble(), hp, htx, hty,
                                isEraser(event, i), 2)
                        }
                        sendPenEvent(event, i, 2, vw, vh)
                    } else {
                        sendFinger(event, i, 2, vw, vh)
                    }
                }
            }

            MotionEvent.ACTION_UP,
            MotionEvent.ACTION_POINTER_UP -> {
                if (isPalm(event, actionIndex)) {
                    return true
                }
                if (isPenLike(event, actionIndex)) {
                    sendPenEvent(event, actionIndex, 1, vw, vh)
                } else {
                    sendFinger(event, actionIndex, 1, vw, vh)
                }
            }

            MotionEvent.ACTION_CANCEL -> {
                // Every pointer is lifted, each on its own device: a cancelled
                // stylus stroke released as a touch would leave the host's pen
                // pressed until the pen next left proximity.
                for (i in 0 until pointerCount) {
                    if (isPalm(event, i)) continue
                    if (isPenLike(event, i) && !penEnabled) continue
                    if (!isPenLike(event, i) && !touchEnabled) continue
                    if (isPenLike(event, i)) {
                        sendPenEvent(event, i, 1, vw, vh)
                    }
                }
                releaseTouches()
            }
        }
        return true
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
            put("x", x)
            put("y", y)
            put("pressure", pressure.coerceIn(0.0, 1.0))
            put("tilt_x", tiltX)
            put("tilt_y", tiltY)
            put("eraser", eraser)
            put("action", action)
        }
        webSocket?.send(msg.toString())
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
        webSocket?.send(msg.toString())
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
        webSocket?.send(msg.toString())
    }

    private fun sendTouch(x: Float, y: Float, pressure: Double,
                          action: Int, slot: Int) {
        if (!touchEnabled) return
        val msg = JSONObject().apply {
            put("type", "touch")
            put("x", x.toDouble())
            put("y", y.toDouble())
            put("pressure", pressure.coerceIn(0.0, 1.0))
            put("action", action)
            put("slot", slot)
        }
        webSocket?.send(msg.toString())
    }

    /**
     * Push encoder settings to the host. The host live-restarts ffmpeg with
     * the new parameters and persists them in its config file. Settings are
     * also remembered here and re-sent on every reconnect.
     */
    fun sendConfig(bitrateKbps: Int, fps: Int) {
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
     * when it emitted that frame, so the round trip it computes is the real
     * end-to-end latency without either side needing a shared time base.
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
        webSocket?.send(msg.toString())
    }

    /**
     * Ask the host to switch between being a second screen and being a
     * graphics tablet. The host applies it and answers with its new mode, so
     * the UI follows [onModeKnown] rather than assuming this succeeded.
     */
    fun sendMode(penOnly: Boolean) {
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
     * MotionEvent.TOOL_TYPE_PALM exists from API 29. The value is stable
     * (6) and older devices simply never report it, so comparing against the
     * number is correct everywhere; the annotation only tells lint that the
     * comparison is deliberate.
     */
    @android.annotation.SuppressLint("WrongConstant")
    private fun isPalm(event: MotionEvent, index: Int): Boolean =
        event.getToolType(index) == TOOL_TYPE_PALM

    fun isControlConnected(): Boolean = isConnected

    @Synchronized fun disconnect() {
        connectionWanted = false
        touchSlots.clear()
        connectionGeneration++
        reconnectJob?.cancel()
        val previous = webSocket
        webSocket = null
        isConnected = false
        previous?.close(1000, "Client closing")
    }
}
