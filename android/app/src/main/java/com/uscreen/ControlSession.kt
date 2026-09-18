package com.uscreen

import android.util.Log
import android.os.SystemClock
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

internal data class RejectedStreamSettings(val reason: String, val bitrate: Int, val fps: Int, val requestGeneration: Long)

/** Owns authentication, host metadata and socket lifetime; never interprets MotionEvents. */
internal class ControlSession(
    private val lock: Any,
    private val input: ControlInputState,
    private val client: WebSocket.Factory,
    private val decoderCapabilities: suspend (Int, Int, Int) -> JSONObject = DecoderCapabilities::report,
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
    private val statistics = ControlStatistics()
    @Volatile private var isConnected = false
    private val authenticatedControl = MutableStateFlow(false)
    val controlConnected = authenticatedControl.asStateFlow()
    private val connection = MutableStateFlow(ControlConnection())
    val connectionState = connection.asStateFlow()
    private var reconnectJob: Job? = null
    private var capabilityJob: Job? = null
    private var capabilityRequest: Triple<Int, Int, Int>? = null

    /** Set from the host's greeting: it is using us as a graphics tablet for
     *  its own screen, so no video will arrive and none should be waited for. */
    @Volatile var isPenOnly = false
        private set
    var onModeKnown: ((penOnly: Boolean) -> Unit)? = null
    var onCodecKnown: ((codec: String) -> Unit)? = null
    var onFpsKnown: ((fps: Int) -> Unit)? = null
    var onSettingsRejected: ((RejectedStreamSettings) -> Unit)? = null
    @Volatile var settingsGeneration = 0L
        private set

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
                if (!sendAuthentication()) return
                if (!sendResolution()) return
                pendingConfig?.let { if (!enqueue(it)) return }
                flushPendingMode()
            }
        }

        override fun onMessage(webSocket: WebSocket, text: String) {
            synchronized(lock) {
                if (isStale(webSocket)) return
                // The host greets with its mode; everything else it might say is
                // ignored, this channel is otherwise ours to talk on.
                try {
                    val o = JSONObject(text)
                    if (o.optString("status") == "settings_rejected") {
                        applySettingsRejection(o)
                        return
                    }
                    applyInputGreeting(o)
                    applyDecoderGreeting(o)
                    requestDecoderCapabilities(webSocket, o)
                    applyModeGreeting(o)
                    acceptGreeting(o)
                } catch (_: Exception) {}
            }
        }

        override fun onClosing(webSocket: WebSocket, code: Int, reason: String) {
            synchronized(lock) {
                if (isStale(webSocket)) return
                resetGreeting()
                webSocket.close(1000, null)
            }
        }

        override fun onClosed(webSocket: WebSocket, code: Int, reason: String) {
            synchronized(lock) {
                if (isStale(webSocket)) return
                this@ControlSession.webSocket = null
                connectionGeneration++
                isConnected = false
                resetGreeting()
                scheduleReconnect()
            }
        }

        override fun onFailure(webSocket: WebSocket, t: Throwable, response: Response?) {
            synchronized(lock) {
                if (isStale(webSocket)) return
                this@ControlSession.webSocket = null
                connectionGeneration++
                isConnected = false
                resetGreeting()
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

    // Authentication must be accepted before any other message is offered.
    private fun sendAuthentication(): Boolean {
        val currentToken = token
        if (currentToken == null) {
            Log.w(TAG, "No session token yet — the host will send one")
            return true
        }
        return enqueue(JSONObject().apply {
            put("type", "auth")
            put("token", currentToken)
        })
    }

    private fun sendResolution(): Boolean {
        if (nativeWidth <= 0 || nativeHeight <= 0) return true
        return enqueue(JSONObject().apply {
            put("type", "resolution")
            put("width", nativeWidth)
            put("height", nativeHeight)
            if (nativeWidthMm > 0 && nativeHeightMm > 0) {
                put("width_mm", nativeWidthMm)
                put("height_mm", nativeHeightMm)
            }
        })
    }

    private fun flushPendingMode() {
        val message = pendingMode ?: return
        // Acceptance means queued, not a host acknowledgement. Preserve the
        // existing one-shot policy once accepted; rejected choices remain pending.
        if (enqueue(message)) pendingMode = null
    }

    private fun resetGreeting() {
        capabilityJob?.cancel()
        capabilityJob = null
        capabilityRequest = null
        connection.value = ControlConnection()
        authenticatedControl.value = false
    }

    private fun acceptGreeting(message: JSONObject) {
        if (message.optString("status") != "connected") return
        connection.value = ControlConnection(true, StreamTransport.from(message.optString("transport")))
        authenticatedControl.value = true
    }

    private fun applyInputGreeting(o: JSONObject) {
        if (o.has("touch")) {
            input.setTouchEnabled(o.getBoolean("touch"))
        }
        if (o.has("pen")) input.setPenEnabled(o.getBoolean("pen"))
    }

    private fun requestDecoderCapabilities(source: WebSocket, message: JSONObject) {
        val width = message.optInt("video_width")
        val height = message.optInt("video_height")
        val fps = message.optInt("fps")
        if (width !in 2..4096 || height !in 2..4096 || fps !in 10..90) return
        val request = Triple(width, height, fps)
        if (capabilityRequest == request) return
        capabilityRequest = request
        capabilityJob?.cancel()
        capabilityJob = scope.launch {
            val report = decoderCapabilities(width, height, fps)
            synchronized(lock) {
                if (isActive && webSocket === source && isConnected && capabilityRequest == request) {
                    enqueue(JSONObject().put("type", "decoders").put("capabilities", report))
                }
            }
        }
    }

    private fun matchesPendingConfig(requested: JSONObject): Boolean {
        val pending = pendingConfig ?: return false
        return listOf("fps", "bitrate").all { field ->
            requested.isNull(field) || requested.optInt(field) == pending.optInt(field)
        }
    }

    private fun applySettingsRejection(message: JSONObject) {
        val requested = message.optJSONObject("requested")
        if (requested != null && !matchesPendingConfig(requested)) return
        val fps = message.optInt("fps")
        val bitrate = message.optInt("bitrate")
        if (fps !in 10..90 || bitrate !in 1000..60000) return
        // Reconnect must resend confirmed settings rather than the rejected request.
        pendingConfig?.put("fps", fps)?.put("bitrate", bitrate)
        onSettingsRejected?.invoke(RejectedStreamSettings(
            message.optString("error", "Unsupported stream settings"), bitrate, fps, settingsGeneration))
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
        resetGreeting()
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
        settingsGeneration++
        val msg = JSONObject().apply {
            put("type", "config")
            put("bitrate", bitrateKbps)
            put("fps", fps)
        }
        pendingConfig = msg
        if (enqueue(msg)) Log.i(TAG, "Queued config: $msg")
    }

    /**
     * Acknowledge the render callback for frame [seq]. The host starts timing
     * when the encoded packet is ready. Its interval includes host queueing,
     * delivery, callback scheduling and the return path, excluding capture and encoding, without
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
            // Complete packet arrival → render callback execution on this device.
            // The host subtracts independent medians as a rough residual estimate;
            // it includes host queueing and the ACK path, not just wire transit.
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
        // Keep the latest choice until the transport accepts it. The host
        // remains authoritative after acceptance, so don't replay it forever.
        pendingMode = msg
        if (isConnected) flushPendingMode()
    }

    fun isControlConnected(): Boolean = isConnected

    // onOpen holds this monitor until auth and initial metadata are queued.
    // UI input and decoder acknowledgements must not overtake that handshake.
    fun sendWhenConnected(message: JSONObject, sampleTimeMs: Long? = null): Unit = synchronized(lock) {
        enqueue(message, sampleTimeMs)
    }

    // Caller holds the same lock as handshake, input translation and settings.
    private fun enqueue(message: JSONObject, sampleTimeMs: Long? = null): Boolean {
        if (!isConnected) return false
        val socket = webSocket ?: return false
        val text = message.toString()
        val age = sampleTimeMs?.let { SystemClock.uptimeMillis() - it }
        val before = socket.queueSize()
        val accepted = socket.send(text)
        statistics.record(accepted, before, socket.queueSize(), age)
        if (accepted) return true
        // A refused ordered event makes the whole gesture uncertain. Stop using
        // this socket, causing the host to release its controller's devices.
        // Never buffer/replay a partial stroke on a replacement connection.
        Log.w(TAG, "Control send rejected; reconnecting (queued_bytes=${socket.queueSize()})")
        webSocket = null
        connectionGeneration++
        isConnected = false
        resetGreeting()
        input.forgetTouches()
        socket.cancel()
        scheduleReconnect()
        return false
    }

    fun statistics(): ControlStatisticsSnapshot = synchronized(lock) {
        statistics.snapshot(webSocket?.queueSize() ?: 0)
    }

    fun disconnect(): Unit = synchronized(lock) {
        if (webSocket != null) Log.i(TAG, "Control statistics: ${statistics()}")
        connectionWanted = false
        input.forgetTouches()
        connectionGeneration++
        reconnectJob?.cancel()
        val previous = webSocket
        webSocket = null
        isConnected = false
        resetGreeting()
        previous?.close(1000, "Client closing")
    }
}
