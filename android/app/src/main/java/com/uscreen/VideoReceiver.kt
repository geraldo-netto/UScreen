package com.uscreen

import android.media.MediaCodec
import android.media.MediaFormat
import android.os.Handler
import android.os.HandlerThread
import android.util.Log
import android.view.Surface
import android.view.SurfaceView
import kotlinx.coroutines.*
import java.io.InputStream
import java.net.Socket
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicLong
import java.util.concurrent.atomic.AtomicReference

class VideoReceiver(private val openSocket: () -> Socket = { Socket(HOST, PORT) }) {
    companion object {
        const val HOST = "127.0.0.1"
        const val PORT = 8890
        const val MIME_TYPE = "video/avc"
        const val MIME_TYPE_HEVC = "video/hevc"
        const val TAG = "UScreenVideo"
        const val MAX_FRAME_SIZE = 8 * 1024 * 1024
        const val PACKET_TYPE_CONFIG = 0
        const val PACKET_TYPE_FRAME = 1

        /** type byte + 4-byte big-endian sequence number */
        const val FRAME_HEADER_SIZE = 5

        /**
         * Acknowledge every Nth rendered frame.
         *
         * Every frame: an idle screen only sends ~5fps, so sampling one in four
         * left 6-12 measurements per report window and percentiles that moved
         * several milliseconds run to run — enough noise to read a regression
         * into pure variance. One small websocket message per frame is
         * negligible next to the pen event rate.
         */
        const val ACK_EVERY = 1

        /** Frames of arrival history kept for the decode-time split. */
        const val ARRIVAL_RING = 64
    }

    private var socket: Socket? = null
    private var inputStream: InputStream? = null
    @Volatile private var mediaCodec: MediaCodec? = null
    private var outputThread: Thread? = null
    @Volatile private var isRunning = false
    private val sessionGeneration = AtomicLong(0)
    private fun isCurrent(generation: Long) = isRunning && sessionGeneration.get() == generation
    @Volatile private var codecAlive = false

    var onConnected: (() -> Unit)? = null
    var onDisconnected: (() -> Unit)? = null

    /**
     * Invoked with the host's frame sequence number once that frame is
     * actually on screen. The host times the round trip on its own clock, so
     * no clock synchronisation between the two devices is needed.
     */
    var onFrameRendered: ((seq: Int, decodeUs: Int) -> Unit)? = null

    /// Which bitstream the host is sending. Set from the host's greeting
    /// before the stream starts; the frames carry nothing that says which
    /// codec they are, so guessing wrong means a decoder that never outputs.
    @Volatile var mimeType: String = MIME_TYPE

    /// Session token; written as the first 64 bytes on the socket. The host
    /// sends nothing until it has seen it.
    @Volatile var token: String? = null

    private var frameCallbackThread: HandlerThread? = null
    internal var callbackThreadFactory: () -> HandlerThread = { HandlerThread("uscreen-frame-cb") }
    private val renderedCount = AtomicLong(0)

    /**
     * Output watchdog. The input-side check below catches a decoder that
     * stops taking frames; it does nothing for one that keeps taking them and
     * never puts any on screen, which is what a Galaxy Tab S10 FE+ on Android
     * 16 did — one frame rendered, then silence, while the host kept sending
     * (issue #10). Frames queued since the last output, and when that was.
     */
    private val queuedSinceOutput = AtomicInteger(0)
    @Volatile private var lastOutputNanos = 0L
    private var outputStalls = 0
    /**
     * Whether to ask for low-latency decoding. Off after the second stall in
     * a row: those hints are exactly the kind of thing a decoder can accept
     * and then misbehave on, and a picture that is late beats no picture.
     */
    @Volatile private var lowLatencyHints = true

    /**
     * seq → nanoTime the frame finished arriving, so the render callback can
     * report the arrival-to-render portion of the host’s send-to-ack interval
     * rather than on the wire. Bounded and cheap: a plain ring, since frames
     * are rendered in the order they arrive.
     */
    private val arrivalSeq = IntArray(ARRIVAL_RING)
    private val arrivalNanos = LongArray(ARRIVAL_RING)
    @Volatile private var arrivalWrite = 0

    /**
     * Splits the on-device time into "decoder produced the frame" and "the
     * compositor put it on screen". Without this the two are indistinguishable,
     * and they call for completely different fixes — decoder settings versus
     * refresh rate and composition path.
     */
    private val releaseNanos = LongArray(ARRIVAL_RING)
    private var decodeSumUs = 0L
    private var presentSumUs = 0L
    private var splitCount = 0
    private var lastSplitLogNanos = 0L

    private fun noteReleased(seq: Int) {
        for (n in 0 until ARRIVAL_RING) {
            val i = (arrivalWrite - 1 - n + ARRIVAL_RING * 2) % ARRIVAL_RING
            if (arrivalSeq[i] == seq) {
                releaseNanos[i] = System.nanoTime()
                return
            }
        }
    }

    private fun noteArrival(seq: Int) {
        val i = arrivalWrite % ARRIVAL_RING
        arrivalSeq[i] = seq
        arrivalNanos[i] = System.nanoTime()
        arrivalWrite = arrivalWrite + 1
    }

    /** Microseconds between the frame arriving and it being on screen, or -1. */
    private fun decodeMicrosFor(seq: Int): Int {
        for (n in 0 until ARRIVAL_RING) {
            val i = (arrivalWrite - 1 - n + ARRIVAL_RING * 2) % ARRIVAL_RING
            if (arrivalSeq[i] == seq && arrivalNanos[i] != 0L) {
                val now = System.nanoTime()
                val total = ((now - arrivalNanos[i]) / 1000L)
                    .coerceIn(0L, Int.MAX_VALUE.toLong()).toInt()

                // Attribute the time: decode = arrival → buffer released,
                // present = released → actually on screen (composition+vsync).
                val rel = releaseNanos[i]
                if (rel > arrivalNanos[i]) {
                    decodeSumUs += (rel - arrivalNanos[i]) / 1000L
                    presentSumUs += (now - rel) / 1000L
                    splitCount++
                    if (now - lastSplitLogNanos > 5_000_000_000L && splitCount > 0) {
                        Log.i(
                            TAG,
                            "on-device split: decode ${decodeSumUs / splitCount / 1000.0}ms " +
                                "present ${presentSumUs / splitCount / 1000.0}ms " +
                                "($splitCount frames)"
                        )
                        lastSplitLogNanos = now
                        decodeSumUs = 0; presentSumUs = 0; splitCount = 0
                    }
                }
                return total
            }
        }
        return -1
    }

    /** Initial decoder format hint; the decoder adapts to the SPS anyway. */
    @Volatile var formatWidth = 1920
    @Volatile var formatHeight = 1080

    /** Frame rate the host is configured to send, used to size decoder hints. */
    @Volatile var streamFps = Prefs.DEFAULT_FPS
        set(value) {
            if (value !in 10..90) return
            synchronized(this) {
                if (field == value) return
                val restart = isRunning
                if (restart) stop()
                field = value
                if (restart) start()
            }
        }

    // Stats
    private val frameCounter = AtomicInteger(0)
    private val byteCounter = AtomicLong(0)
    @Volatile var currentFps = 0f; private set
    @Volatile var currentMbps = 0f; private set

    private val surfaceReady = AtomicBoolean(false)
    private val pendingSurface = AtomicReference<Surface?>(null)

    /**
     * Recreated on every [start].
     *
     * These must NOT be `val`s initialised once: [stop] cancels the job, and a
     * cancelled [SupervisorJob] stays cancelled forever, so every later
     * `scope.launch {}` returns an already-dead coroutine whose body never
     * runs. That is what left the tablet on a black screen after the app had
     * been backgrounded once — the only cure was force-stopping it.
     */
    private var job: Job? = null
    private var scope: CoroutineScope? = null

    fun setSurface(surfaceView: SurfaceView) {
        val surface = surfaceView.holder.surface
        if (surface == null || !surface.isValid) {
            Log.w(TAG, "Surface not ready yet")
            return
        }
        pendingSurface.set(surface)
        surfaceReady.set(true)
        Log.i(TAG, "Surface stored, ready for codec setup")

        // Only while receiving. Building a decoder for a surface that shows
        // nothing (no host yet, or pen-only mode, where this receiver is
        // stopped on purpose) costs a hardware codec and a polling thread
        // that nothing releases until the next stop(); start() sets the
        // codec up itself once it runs.
        synchronized(this) {
            if (isRunning && mediaCodec == null && surfaceReady.get()) {
                setupCodec(surface)
            }
        }
    }

    /**
     * The surface backing the decoder is going away. Release the codec here
     * rather than letting it keep rendering into a destroyed surface, which
     * throws from the render thread on the way to the background.
     */
    @Synchronized fun onSurfaceDestroyed() {
        surfaceReady.set(false)
        pendingSurface.set(null)
        resetCodec()
    }

    @Synchronized private fun setupCodec(surface: Surface): Boolean {
        var pendingCodec: MediaCodec? = null
        var pendingThread: HandlerThread? = null
        try {
            val format = decoderFormat()
            queuedSinceOutput.set(0)
            lastOutputNanos = System.nanoTime()

            val codec = MediaCodec.createDecoderByType(mimeType)
            pendingCodec = codec
            codec.configure(format, surface, null, 0)
            codec.setVideoScalingMode(MediaCodec.VIDEO_SCALING_MODE_SCALE_TO_FIT)

            // Fires when a frame has actually reached the output surface —
            // the true "it is on screen" moment, rather than the earlier
            // moment we handed the buffer back. The host's sequence number
            // rides along as the presentation timestamp.
            val cbThread = callbackThreadFactory()
            pendingThread = cbThread
            cbThread.start()
            codec.setOnFrameRenderedListener({ _, presentationTimeUs, _ ->
                if (mediaCodec === codec && codecAlive && isRunning && renderedCount.incrementAndGet() % ACK_EVERY == 0L) {
                    val seq = presentationTimeUs.toInt()
                    onFrameRendered?.invoke(seq, decodeMicrosFor(seq))
                }
            }, Handler(cbThread.looper))

            codec.start()
            mediaCodec = codec
            frameCallbackThread = cbThread
            codecAlive = true
            startOutputThread(codec)
            Log.i(TAG, "Codec configured and started with surface")
            return true
        } catch (e: Exception) {
            Log.e(TAG, "Failed to setup codec", e)
            // Ownership transfers only after successful startup. A failure at
            // configure/listener/start must retire these locals before retry.
            if (mediaCodec === pendingCodec) {
                codecAlive = false
                mediaCodec = null
            }
            if (frameCallbackThread === pendingThread) frameCallbackThread = null
            pendingCodec?.let {
                try { it.stop() } catch (_: Exception) {}
                try { it.release() } catch (_: Exception) {}
            }
            pendingThread?.let {
                it.quitSafely()
                if (Thread.currentThread() !== it) it.join(500)
            }
            return false
        }
    }

    private fun decoderFormat(): MediaFormat {
        val format = MediaFormat.createVideoFormat(mimeType, formatWidth, formatHeight)
        // Follow the stream's real frame rate rather than a hardcoded
        // guess: telling the decoder 90 when the host sends 60 skews its
        // internal pacing and power/clock decisions.
        format.setInteger(MediaFormat.KEY_FRAME_RATE, streamFps)
        format.setInteger(MediaFormat.KEY_I_FRAME_INTERVAL, 1)

        // State the colour space explicitly rather than relying on the SPS
        // alone. A/B measured: no latency cost either way, and being
        // explicit means the decoder cannot guess wrong.
        try {
            format.setInteger(MediaFormat.KEY_COLOR_RANGE, MediaFormat.COLOR_RANGE_LIMITED)
            format.setInteger(MediaFormat.KEY_COLOR_STANDARD, MediaFormat.COLOR_STANDARD_BT709)
            format.setInteger(MediaFormat.KEY_COLOR_TRANSFER, MediaFormat.COLOR_TRANSFER_SDR_VIDEO)
        } catch (_: Exception) {}

        // Low latency flags (safe to set, ignored if unsupported) — unless
        // this decoder has already stalled on them, see lowLatencyHints.
        if (lowLatencyHints) {
            if (android.os.Build.VERSION.SDK_INT >= 30) {
                format.setInteger(MediaFormat.KEY_LOW_LATENCY, 1)
            }
            try {
                // Ask the decoder to run flat out rather than pace to the
                // frame rate — headroom above the stream rate, so a late
                // frame is caught up on instead of waiting for the next slot.
                format.setInteger("operating-rate", streamFps * 2)
            } catch (_: Exception) {}
            try {
                format.setInteger("vendor.qti-ext-dec-low-latency.enable", 1)
            } catch (_: Exception) {}
        } else {
            Log.w(TAG, "Configuring decoder without low-latency hints")
        }
        return format
    }

    /**
     * Dedicated render thread: drains decoded frames and releases them to the
     * surface as soon as they're ready, independent of network reads. This is
     * what keeps the display latency at "one frame", not "one network stall".
     */
    private fun startOutputThread(codec: MediaCodec) {
        outputThread = Thread({
            val info = MediaCodec.BufferInfo()
            var rendered = 0L
            while (codecAlive && mediaCodec === codec) {
                try {
                    val index = codec.dequeueOutputBuffer(info, 10_000) // 10ms
                    if (mediaCodec !== codec || !codecAlive) break
                    if (index >= 0) {
                        val seq = info.presentationTimeUs.toInt()
                        codec.releaseOutputBuffer(index, true)
                        lastOutputNanos = System.nanoTime()
                        queuedSinceOutput.set(0)
                        outputStalls = 0
                        noteReleased(seq)
                        frameCounter.incrementAndGet()
                        rendered++
                        if (rendered <= 2) Log.i(TAG, "Rendered output frame #$rendered")
                    }
                } catch (e: IllegalStateException) {
                    retireFailedOutput(codec, "Output thread: codec gone", e)
                    break
                } catch (e: Exception) {
                    retireFailedOutput(codec, "Output thread error", e)
                    break
                }
            }
        }, "uscreen-render").apply {
            priority = Thread.MAX_PRIORITY
            start()
        }
    }

    private fun retireFailedOutput(codec: MediaCodec, message: String, error: Exception) {
        if (codecAlive) Log.w(TAG, message, error)
        synchronized(this) {
            if (mediaCodec === codec) resetCodec()
        }
    }

    fun start() {
        synchronized(this) {
            if (isRunning) return
            isRunning = true
            val generation = sessionGeneration.incrementAndGet()
            val sessionToken = token
            // Fresh job/scope per start — see the field docs.
            val newJob = SupervisorJob()
            val newScope = CoroutineScope(Dispatchers.IO + newJob)
            job = newJob
            scope = newScope

            newScope.launch {
                connectAndReceive(generation, sessionToken)
            }

            newScope.launch {
                while (isCurrent(generation)) {
                    delay(1000)
                    if (!isCurrent(generation)) break
                    currentFps = frameCounter.getAndSet(0).toFloat()
                    currentMbps = byteCounter.getAndSet(0) * 8f / 1_000_000f
                }
            }
        }
    }

    private suspend fun connectAndReceive(generation: Long, sessionToken: String?) {
        while (isCurrent(generation)) {
            var sessionSocket: Socket? = null
            try {
                awaitSurface(generation)
                if (!isCurrent(generation)) return
                val codecReady = synchronized(this@VideoReceiver) {
                    if (!isCurrent(generation)) return
                    ensureSurfaceCodec()
                }
                if (!codecReady) {
                    Log.w(TAG, "Codec/surface not ready, retrying...")
                    delay(500)
                    continue
                }
                Log.i(TAG, "Connecting to $HOST:$PORT...")
                val connection = openSocket()
                sessionSocket = connection
                connection.apply {
                    tcpNoDelay = true
                    soTimeout = 10000 // 10s read timeout
                    // Small on purpose. A 1 MB receive buffer let the host run
                    // ahead and park whole frames here, where they are pure
                    // delay that neither side can see or skip past. Keeping it
                    // shallow pushes backpressure back to the host, which does
                    // know how to drop stale frames.
                    receiveBufferSize = 128 * 1024
                }
                val input = connection.getInputStream()
                synchronized(this@VideoReceiver) {
                    if (!isCurrent(generation)) return
                    socket = connection
                    inputStream = input
                }
                sessionToken?.let { t ->
                    connection.getOutputStream().apply {
                        write(t.toByteArray(Charsets.US_ASCII))
                        flush()
                    }
                }
                Log.i(TAG, "Connected to video stream")

                receivePackets(generation, connection, input)
                // Protocol rejection and decoder retirement return normally.
                // They still end the visible connection, just like EOF does.
                disconnectAndPause(generation, 500) { Log.i(TAG, "Stream retired, reconnecting") }
            } catch (e: java.io.EOFException) {
                disconnectAndPause(generation, 1000) { Log.i(TAG, "Stream ended (server closed)") }
            } catch (e: java.net.SocketTimeoutException) {
                disconnectAndPause(generation, 500) { Log.w(TAG, "Stream read timeout, reconnecting") }
            } catch (e: Exception) {
                disconnectAndPause(generation, 1000) { Log.e(TAG, "Stream error: ${e.message}") }
            } finally {
                retireSessionSocket(sessionSocket)
            }
        }
    }

    private suspend fun awaitSurface(generation: Long) {
        while (isCurrent(generation) && !surfaceReady.get()) {
            Log.d(TAG, "Waiting for surface...")
            delay(200)
        }
    }

    // Caller holds the receiver monitor so surface/codec ownership stays atomic.
    private fun ensureSurfaceCodec(): Boolean {
        if (mediaCodec != null) return true
        val surface = pendingSurface.get()
        return if (surface != null && surface.isValid) setupCodec(surface) else false
    }

    private suspend fun disconnectAndPause(generation: Long, pauseMs: Long, report: () -> Unit) {
        if (isCurrent(generation)) {
            report()
            synchronized(this@VideoReceiver) {
                if (isCurrent(generation)) onDisconnected?.invoke()
            }
            delay(pauseMs)
        }
    }

    private fun retireSessionSocket(sessionSocket: Socket?) {
        try { sessionSocket?.close() } catch (_: Exception) {}
        synchronized(this) {
            if (socket === sessionSocket) {
                socket = null
                inputStream = null
            }
        }
    }

    private fun receivePackets(generation: Long, connection: Socket, input: InputStream) {
        val sizeHeader = ByteArray(4)
        // Reuse storage across frames to avoid multi-megabyte allocations at 60 Hz.
        var packetBuf = ByteArray(512 * 1024)
        val packets = VideoPackets(generation)
        while (isCurrent(generation) && !connection.isClosed) {
            val codec = synchronized(this@VideoReceiver) {
                if (isCurrent(generation)) mediaCodec else null
            } ?: break

            readExact(input, sizeHeader, 4)

            val frameSize = ((sizeHeader[0].toInt() and 0xFF) shl 24) or
                    ((sizeHeader[1].toInt() and 0xFF) shl 16) or
                    ((sizeHeader[2].toInt() and 0xFF) shl 8) or
                    (sizeHeader[3].toInt() and 0xFF)

            if (frameSize <= 1 || frameSize > MAX_FRAME_SIZE + 1) {
                Log.w(TAG, "Invalid packet size: $frameSize, reconnecting")
                break // Reconnect
            }

            if (packetBuf.size < frameSize) {
                packetBuf = ByteArray(frameSize + frameSize / 2)
            }
            readExact(input, packetBuf, frameSize)
            if (!isCurrent(generation)) break
            byteCounter.addAndGet(frameSize.toLong())
            if (!packets.handle(codec, packetBuf, frameSize)) break
        }
    }

    private inner class VideoPackets(private val generation: Long) {
        private var firstFrame = true

        fun handle(codec: MediaCodec, data: ByteArray, size: Int): Boolean {
            val packetType = data[0].toInt() and 0xFF
            when (packetType) {
                PACKET_TYPE_CONFIG -> {
                    val payloadSize = size - 1
                    Log.i(TAG, "Received codec config: ${payloadSize}B")
                    feedDecoder(generation, codec, data, 1, payloadSize, true, 0L)
                }
                PACKET_TYPE_FRAME -> {
                    if (size <= FRAME_HEADER_SIZE) {
                        Log.w(TAG, "Truncated frame packet: $size, reconnecting")
                        return false
                    }
                    deliverFrame(codec, data, size)
                }
                else -> {
                    Log.w(TAG, "Unknown packet type: $packetType, reconnecting")
                    return false
                }
            }
            return true
        }

        private fun deliverFrame(codec: MediaCodec, data: ByteArray, size: Int) {
            // 4-byte big-endian sequence number after the type
            // byte, carried through the decoder as the
            // presentation timestamp and echoed to the host.
            val seq = ((data[1].toInt() and 0xFF) shl 24) or
                    ((data[2].toInt() and 0xFF) shl 16) or
                    ((data[3].toInt() and 0xFF) shl 8) or
                    (data[4].toInt() and 0xFF)
            if (firstFrame) {
                firstFrame = false
                synchronized(this@VideoReceiver) {
                    if (isCurrent(generation)) onConnected?.invoke()
                }
            }
            noteArrival(seq)
            feedDecoder(
                generation, codec, data, FRAME_HEADER_SIZE,
                size - FRAME_HEADER_SIZE, false,
                seq.toLong() and 0xFFFFFFFFL
            )
        }
    }

    /**
     * Queue one access unit into the decoder. Never silently drops frames:
     * a dropped P-frame corrupts the picture until the next keyframe. If no
     * input buffer frees up within ~200ms the codec is genuinely stuck and we
     * reset it instead.
     */
    @Synchronized private fun feedDecoder(
        generation: Long, codec: MediaCodec, data: ByteArray, offset: Int, size: Int,
        isConfig: Boolean, presentationTimeUs: Long
    ) {
        if (!isCurrent(generation) || mediaCodec !== codec) return
        try {
            var attempts = 0
            while (true) {
                val inputIndex = codec.dequeueInputBuffer(20_000) // 20ms
                if (inputIndex >= 0) {
                    val inputBuffer = checkNotNull(codec.getInputBuffer(inputIndex)) {
                        "Decoder returned no input buffer"
                    }
                    inputBuffer.clear()
                    inputBuffer.put(data, offset, size)

                    val flags = if (isConfig) MediaCodec.BUFFER_FLAG_CODEC_CONFIG else 0
                    // The host's sequence number rides in the presentation
                    // timestamp so the render callback can identify the frame.
                    codec.queueInputBuffer(
                        inputIndex,
                        0,
                        size,
                        presentationTimeUs,
                        flags
                    )
                    if (!isConfig) {
                        checkOutputProgress()
                    }
                    return
                }
                attempts++
                if (attempts >= 10) {
                    Log.w(TAG, "Decoder stuck for 200ms — resetting codec")
                    resetCodec()
                    return
                }
            }
        } catch (e: MediaCodec.CodecException) {
            Log.e(TAG, "Decoder codec error: ${e.diagnosticInfo}", e)
            resetCodec()
        } catch (e: Exception) {
            Log.w(TAG, "Decoder feed error", e)
            resetCodec()
        }
    }

    private fun checkOutputProgress() {
        val queued = queuedSinceOutput.incrementAndGet()
        val silentNs = System.nanoTime() - lastOutputNanos
        if (queued >= 4 && silentNs > 1_500_000_000L) {
            outputStalls++
            val dropHints = outputStalls >= 2 && lowLatencyHints
            Log.w(
                TAG,
                "Decoder took $queued frames and showed none for " +
                    "${silentNs / 1_000_000} ms — restarting" +
                    (if (dropHints) " without low-latency hints" else "")
            )
            if (dropHints) lowLatencyHints = false
            // A fresh decoder needs the codec config and a
            // keyframe again, and the host sends both to a
            // client that (re)connects — so drop the socket
            // too, and let the read loop come back.
            resetCodec()
        }
    }

    /** Tear the decoder down without touching the surface or the socket. */
    private fun releaseCodec() {
        synchronized(this) {
            codecAlive = false
            outputThread?.let { if (it !== Thread.currentThread()) it.join(500) }
            outputThread = null
            mediaCodec?.let {
                try { it.stop() } catch (_: Exception) {}
                try { it.release() } catch (_: Exception) {}
            }
            mediaCodec = null
            frameCallbackThread?.quitSafely()
            frameCallbackThread = null
        }
    }

    private fun resetCodec() {
        synchronized(this) {
            // Every replacement decoder needs headers and a keyframe. Leave
            // creation to reconnect, which obtains both from the host.
            try { socket?.close() } catch (_: Exception) {}
            releaseCodec()
        }
    }

    private fun readExact(stream: InputStream, buffer: ByteArray, length: Int) {
        var offset = 0
        while (offset < length) {
            val read = stream.read(buffer, offset, length - offset)
            if (read < 0) throw java.io.EOFException("Stream closed")
            offset += read
        }
    }

    fun getFps(): Float = currentFps
    fun getMbps(): Float = currentMbps

    @Synchronized fun stop() {
        sessionGeneration.incrementAndGet()
        isRunning = false
        codecAlive = false
        // Close socket first to unblock any pending reads
        try {
            socket?.close()
        } catch (_: Exception) {}
        socket = null
        inputStream = null

        // Then cancel coroutines. The job is dropped rather than reused: a new
        // one is created by the next start().
        job?.cancel()
        job = null
        scope = null

        releaseCodec()

        // The surface is deliberately left alone. It belongs to the
        // SurfaceView, which outlives any single streaming session — it stays
        // in the view tree while the tablet is a graphics tablet, so no
        // surfaceCreated callback ever comes to hand it back. Clearing it here
        // left the next start() waiting on a surface that would never arrive,
        // showing the last decoded frame frozen on screen.
    }
}
