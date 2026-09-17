package com.uscreen

import android.media.MediaCodec
import android.util.Log
import android.view.Surface
import android.view.SurfaceView
import kotlinx.coroutines.*
import java.io.InputStream
import java.net.Socket
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong
import java.util.concurrent.atomic.AtomicReference

class VideoReceiver(createSocket: () -> Socket = { Socket() }) {
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

    @Volatile private var isRunning = false
    private val sessionGeneration = AtomicLong(0)
    private fun isCurrent(generation: Long) = isRunning && sessionGeneration.get() == generation

    private var videoConnected = false // Guarded by the receiver monitor.
    var onConnected: (() -> Unit)? = null
    var onDisconnected: (() -> Unit)? = null

    // Install both callbacks and replay readiness atomically: the first frame
    // may precede Compose's effect, and retirement may race its registration.
    @Synchronized fun observeConnection(connected: () -> Unit, disconnected: () -> Unit) {
        onConnected = connected
        onDisconnected = disconnected
        if (videoConnected) connected() else disconnected()
    }

    /**
     * Invoked with the host's sequence when its render notification is delivered,
     * not a physical-screen timestamp. The host times the round trip on its clock, so
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

    internal val transport = VideoTransport(this, ::isCurrent, createSocket)
    internal val timing = FrameTiming()
    internal val decoder = DecoderSession(this, { isRunning }, timing, object : DecoderEvents {
        override fun rendered(sequence: Int, decodeMicros: Int) {
            onFrameRendered?.invoke(sequence, decodeMicros)
        }
        override fun invalidated() {
            transport.interrupt()
        }
    }, { statistics })

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

    @Volatile private var statistics = ReceiverStatistics()
    val currentFps get() = statistics.fps
    val currentMbps get() = statistics.mbps

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
            if (isRunning && decoder.mediaCodec == null && surfaceReady.get()) {
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
        decoder.resetCodec()
    }

    fun start() {
        synchronized(this) {
            if (isRunning) return
            isRunning = true
            videoConnected = false
            val generation = sessionGeneration.incrementAndGet()
            val sessionToken = token
            val sessionStatistics = ReceiverStatistics()
            statistics = sessionStatistics
            // Fresh job/scope per start — see the field docs.
            val newJob = SupervisorJob()
            val newScope = CoroutineScope(Dispatchers.IO + newJob)
            job = newJob
            scope = newScope

            newScope.launch {
                connectAndReceive(generation, sessionToken, sessionStatistics)
            }

            newScope.launch {
                while (isCurrent(generation)) {
                    delay(1000)
                    if (!isCurrent(generation)) break
                    sessionStatistics.sample()
                }
            }
        }
    }

    private suspend fun connectAndReceive(
        generation: Long, sessionToken: String?, sessionStatistics: ReceiverStatistics
    ) {
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
                val connection = transport.newConnection()
                sessionSocket = connection
                val input = transport.connect(generation, connection) ?: return
                sessionToken?.let { t ->
                    connection.getOutputStream().apply {
                        write(t.toByteArray(Charsets.US_ASCII))
                        flush()
                    }
                }
                Log.i(TAG, "Connected to video stream")

                receivePackets(generation, connection, input, sessionStatistics)
                // Protocol rejection and decoder retirement return normally.
                // They still end the visible connection, just like EOF does.
                disconnectAndPause(generation, 500) { Log.i(TAG, "Stream retired, reconnecting") }
            } catch (e: java.io.EOFException) {
                disconnectAndPause(generation, 1000) { Log.i(TAG, "Stream ended (server closed)") }
            } catch (e: java.net.SocketTimeoutException) {
                disconnectAndPause(generation, 500) { Log.w(TAG, "Video socket timeout, reconnecting") }
            } catch (e: Exception) {
                disconnectAndPause(generation, 1000) { Log.e(TAG, "Stream error: ${e.message}") }
            } finally {
                transport.retire(sessionSocket)
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
        if (decoder.mediaCodec != null) return true
        val surface = pendingSurface.get()
        return if (surface != null && surface.isValid) setupCodec(surface) else false
    }

    private suspend fun disconnectAndPause(generation: Long, pauseMs: Long, report: () -> Unit) {
        if (isCurrent(generation)) {
            report()
            synchronized(this@VideoReceiver) {
                if (isCurrent(generation)) {
                    videoConnected = false
                    onDisconnected?.invoke()
                }
            }
            delay(pauseMs)
        }
    }

    private fun receivePackets(
        generation: Long, connection: Socket, input: InputStream, sessionStatistics: ReceiverStatistics
    ) {
        val reader = VideoPacketReader(input)
        val packets = VideoPackets(generation)
        while (isCurrent(generation) && !connection.isClosed) {
            val codec = synchronized(this@VideoReceiver) {
                if (isCurrent(generation)) decoder.mediaCodec else null
            } ?: break

            if (!reader.read() || !isCurrent(generation)) break
            sessionStatistics.bytesReceived(reader.size)
            if (!packets.handle(codec, reader)) break
        }
    }

    private inner class VideoPackets(private val generation: Long) : VideoPacketSink {
        private var firstFrame = true
        private lateinit var codec: MediaCodec

        fun handle(codec: MediaCodec, reader: VideoPacketReader): Boolean {
            this.codec = codec
            return reader.dispatch(this)
        }

        override fun configuration(data: ByteArray, offset: Int, size: Int) {
            Log.i(TAG, "Received codec config: ${size}B")
            feedDecoder(generation, codec, data, offset, size, true, 0L)
        }

        override fun frame(sequence: Int, data: ByteArray, offset: Int, size: Int) {
            val seq = sequence
            if (firstFrame) {
                firstFrame = false
                synchronized(this@VideoReceiver) {
                    if (isCurrent(generation)) {
                        videoConnected = true
                        onConnected?.invoke()
                    }
                }
            }
            feedDecoder(
                generation, codec, data, offset, size, false,
                seq.toLong() and 0xFFFFFFFFL
            )
        }
    }

    @Synchronized internal fun setupCodec(surface: Surface): Boolean =
        decoder.setupCodec(surface, DecoderFormat(mimeType, formatWidth, formatHeight, streamFps))

    internal fun feedDecoder(
        generation: Long, codec: MediaCodec, data: ByteArray, offset: Int, size: Int,
        isConfig: Boolean, presentationTimeUs: Long, arrivalNanos: Long = System.nanoTime(),
    ) {
        synchronized(this) {
            if (!isCurrent(generation) || decoder.mediaCodec !== codec) return
        }
        decoder.feedDecoder(codec, data, offset, size, isConfig, presentationTimeUs, arrivalNanos)
    }

    fun getFps(): Float = currentFps
    fun getMbps(): Float = currentMbps

    @Synchronized fun stop() {
        val wasRunning = isRunning
        sessionGeneration.incrementAndGet()
        isRunning = false
        videoConnected = false
        decoder.retireOutput()
        // Close socket first to unblock a pending connect or read.
        transport.stop()

        // Then cancel coroutines. The job is dropped rather than reused: a new
        // one is created by the next start().
        job?.cancel()
        job = null
        scope = null

        decoder.releaseCodec()
        // Retired network/render/timer workers keep only their old accumulator.
        statistics = ReceiverStatistics()
        if (wasRunning) onDisconnected?.invoke()

        // The surface is deliberately left alone. It belongs to the
        // SurfaceView, which outlives any single streaming session — it stays
        // in the view tree while the tablet is a graphics tablet, so no
        // surfaceCreated callback ever comes to hand it back. Clearing it here
        // left the next start() waiting on a surface that would never arrive,
        // showing the last decoded frame frozen on screen.
    }
}
