package com.uscreen

import android.util.Log
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import okhttp3.*
import org.json.JSONObject
import java.util.concurrent.TimeUnit

internal fun defaultControlClient(): OkHttpClient = OkHttpClient.Builder()
    .readTimeout(0, TimeUnit.SECONDS)
    .connectTimeout(5, TimeUnit.SECONDS)
    .pingInterval(5, TimeUnit.SECONDS)
    .build()

/** Motion state changed by a connection reset or host greeting, under the shared input lock. */
internal interface ControlInputState {
    fun reset()
    fun forgetTouches()
    fun setTouchEnabled(enabled: Boolean)
    fun setPenEnabled(enabled: Boolean)
}

/** Owns authentication, host metadata and socket lifetime; never interprets MotionEvents. */
internal class ControlSession(
    private val lock: Any,
    private val input: ControlInputState,
    private val client: WebSocket.Factory,
) {
    private companion object {
        const val TAG = TouchCapture.TAG
        const val WS_URL = TouchCapture.WS_URL
        const val RECONNECT_DELAY_MS = TouchCapture.RECONNECT_DELAY_MS
    }

    @Volatile private var webSocket: WebSocket? = null
    @Volatile var connectionGeneration = 0L
        private set
    private var connectionWanted = false
    @Volatile private var isConnected = false
    private val authenticatedControl = MutableStateFlow(false)
    val controlConnected = authenticatedControl.asStateFlow()
    private var reconnectJob: Job? = null

    /** Set from the host's greeting: it is using us as a graphics tablet for
     *  its own screen, so no video will arrive and none should be waited for. */
    @Volatile var isPenOnly = false
        private set
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

    private val scope = CoroutineScope(Dispatchers.IO + SupervisorJob())

    private val wsListener = object : WebSocketListener() {
        override fun onOpen(webSocket: WebSocket, response: Response) {
            synchronized(lock) {
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
            synchronized(lock) {
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
            synchronized(lock) {
                if (isStale(webSocket)) return
                authenticatedControl.value = false
                webSocket.close(1000, null)
            }
        }

        override fun onClosed(webSocket: WebSocket, code: Int, reason: String) {
            synchronized(lock) {
                if (isStale(webSocket)) return
                this@ControlSession.webSocket = null
                connectionGeneration++
                isConnected = false
                authenticatedControl.value = false
                scheduleReconnect()
            }
        }

        override fun onFailure(webSocket: WebSocket, t: Throwable, response: Response?) {
            synchronized(lock) {
                if (isStale(webSocket)) return
                this@ControlSession.webSocket = null
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
        private fun isStale(ws: WebSocket) = ws !== this@ControlSession.webSocket
    }

    private fun applyInputGreeting(o: JSONObject) {
        if (o.has("touch")) {
            input.setTouchEnabled(o.getBoolean("touch"))
        }
        if (o.has("pen")) input.setPenEnabled(o.getBoolean("pen"))
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

    fun connect(): Unit = synchronized(lock) {
        connectionWanted = true
        // Idempotent: a second connect() must not leave the first socket
        // alive with its listener still flipping isConnected. onStart and a
        // token delivered through onNewIntent can both call this.
        if (isConnected) return@synchronized
        // A reconnect already scheduled by onClosed would open a second
        // socket next to this one; this call supersedes it.
        reconnectJob?.cancel()
        webSocket?.cancel()
        webSocket = null
        connectWebSocket()
    }

    private fun connectWebSocket(): Unit = synchronized(lock) {
        connectionGeneration++
        authenticatedControl.value = false
        input.reset()
        val previous = webSocket
        webSocket = null
        previous?.cancel()
        val request = Request.Builder()
            .url(WS_URL)
            .build()
        webSocket = client.newWebSocket(request, wsListener)
    }

    private fun scheduleReconnect(): Unit = synchronized(lock) {
        reconnectJob?.cancel()
        val generation = connectionGeneration
        reconnectJob = scope.launch {
            delay(RECONNECT_DELAY_MS)
            synchronized(lock) {
                if (connectionWanted && generation == connectionGeneration && !isConnected) {
                    connectWebSocket()
                }
            }
        }
    }

    /**
     * Push encoder settings to the host. The host live-restarts ffmpeg with
     * the new parameters and persists them in its config file. Settings are
     * also remembered here and re-sent on every reconnect.
     */
    fun sendConfig(bitrateKbps: Int, fps: Int): Unit = synchronized(lock) {
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
    fun sendMode(penOnly: Boolean): Unit = synchronized(lock) {
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

    fun isControlConnected(): Boolean = isConnected

    // onOpen holds this monitor until auth and initial metadata are queued.
    // UI input and decoder acknowledgements must not overtake that handshake.
    fun sendWhenConnected(message: JSONObject): Unit = synchronized(lock) {
        if (isConnected) webSocket?.send(message.toString())
    }

    fun disconnect(): Unit = synchronized(lock) {
        connectionWanted = false
        input.forgetTouches()
        connectionGeneration++
        reconnectJob?.cancel()
        val previous = webSocket
        webSocket = null
        isConnected = false
        authenticatedControl.value = false
        previous?.close(1000, "Client closing")
    }
}
