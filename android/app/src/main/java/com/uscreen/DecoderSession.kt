package com.uscreen

import android.media.MediaCodec
import android.media.MediaFormat
import android.os.Handler
import android.os.HandlerThread
import android.util.Log
import android.view.Surface
import java.util.concurrent.atomic.AtomicLong
import com.uscreen.VideoReceiver.Companion.ACK_EVERY
import com.uscreen.VideoReceiver.Companion.TAG

internal data class DecoderFormat(val mimeType: String, val width: Int, val height: Int, val fps: Int)

internal interface DecoderEvents {
    fun rendered(sequence: Int, decodeMicros: Int)
    fun invalidated()
}

/** Owns one codec and both output threads. The monitor is shared with the
 * receiver so Surface, generation and codec handoffs remain one transaction. */
internal class DecoderSession(
    private val monitor: Any,
    private val running: () -> Boolean,
    private val timing: FrameTiming,
    private val events: DecoderEvents,
    private val statistics: () -> ReceiverStatistics,
) {
    @Volatile var mediaCodec: MediaCodec? = null; private set
    @Volatile private var codecAlive = false
    private var outputThread: Thread? = null
    private var frameCallbackThread: HandlerThread? = null
    var callbackThreadFactory: () -> HandlerThread = { HandlerThread("uscreen-frame-cb") }
    var createCodec: (String) -> MediaCodec = MediaCodec::createDecoderByType
    var outputClock: () -> Long = System::nanoTime
    private val renderedCount = AtomicLong(0)
    private val outputWatchdog = DecoderOutputWatchdog()
    private var timingEpoch = timing.currentEpoch()

    fun setupCodec(surface: Surface, parameters: DecoderFormat): Boolean {
        synchronized(monitor) {
            val codecTiming = timing.beginEpoch()
            var pendingCodec: MediaCodec? = null
            var pendingThread: HandlerThread? = null
            try {
                val format = decoderFormat(parameters)
                outputWatchdog.restarted(outputClock())

                val codec = createCodec(parameters.mimeType)
                pendingCodec = codec
                codec.configure(format, surface, null, 0)
                codec.setVideoScalingMode(MediaCodec.VIDEO_SCALING_MODE_SCALE_TO_FIT)

                // Acknowledge MediaCodec's render notification. Callback delivery can
                // be delayed or batched, so its execution is not a physical-screen
                // timestamp. The host sequence travels as the presentation timestamp;
                // the separate MediaCodec render-time argument is currently unused.
                val cbThread = callbackThreadFactory()
                pendingThread = cbThread
                cbThread.start()
                codec.setOnFrameRenderedListener({ _, presentationTimeUs, _ ->
                    notifyRendered(codec, codecTiming, presentationTimeUs.toInt())
                }, Handler(cbThread.looper))

                codec.start()
                timingEpoch = codecTiming
                mediaCodec = codec
                frameCallbackThread = cbThread
                codecAlive = true
                startOutputThread(codec, codecTiming)
                Log.i(TAG, "Codec configured and started with surface")
                return true
            } catch (e: Exception) {
                timing.retire(codecTiming)
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
    }

    private fun notifyRendered(codec: MediaCodec, codecTiming: FrameTiming.Epoch, sequence: Int) {
        // Timing/log work can yield. Validate again at acknowledgement so a
        // callback that overlapped replacement cannot target the new session.
        val decodeMicros = timing.decodeMicrosFor(sequence, codecTiming)
        synchronized(monitor) {
            if (mediaCodec !== codec || !codecAlive || !running()) return
            if (renderedCount.incrementAndGet() % ACK_EVERY == 0L) events.rendered(sequence, decodeMicros)
        }
    }

    private fun decoderFormat(parameters: DecoderFormat): MediaFormat {
        val (mimeType, formatWidth, formatHeight, streamFps) = parameters
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
        // this receiver has already fallen back after repeated output stalls.
        if (outputWatchdog.lowLatencyHints) {
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
     * surface as soon as they're ready, independent of network reads. This
     * reduces coupling to network stalls; it does not impose a latency bound.
     */
    fun startOutputThread(codec: MediaCodec, codecTiming: FrameTiming.Epoch = timingEpoch) {
        val outputStatistics = statistics()
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
                        outputWatchdog.output(outputClock())
                        timing.noteReleased(seq, codecTiming)
                        outputStatistics.frameRendered()
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
        synchronized(monitor) {
            if (mediaCodec === codec) resetCodec()
        }
    }

    /**
     * Queue one access unit into the decoder. Never silently drops frames:
     * a dropped P-frame corrupts the picture until the next keyframe. If no
     * input buffer frees up within ~200ms the codec is genuinely stuck and we
     * reset it instead.
     */
    fun feedDecoder(
        codec: MediaCodec, data: ByteArray, offset: Int, size: Int,
        isConfig: Boolean, presentationTimeUs: Long, arrivalNanos: Long = System.nanoTime()
    ) {
        synchronized(monitor) {
            if (mediaCodec !== codec) return
            if (!isConfig) timing.noteArrival(presentationTimeUs.toInt(), timingEpoch, arrivalNanos)
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
    }

    fun checkOutputProgress() {
        val stall = outputWatchdog.queued(outputClock()) ?: return
        Log.w(
            TAG,
            "Decoder took ${stall.queued} frames and showed none for " +
                "${stall.silentNanos / 1_000_000} ms — restarting" +
                (if (stall.droppedHints) " without low-latency hints" else "")
        )
        // A fresh decoder needs config and a keyframe; reconnect obtains both.
        resetCodec()
    }

    fun retireOutput() { codecAlive = false }

    /** Tear the decoder down without touching the surface or the socket. */
    fun releaseCodec() {
        synchronized(monitor) {
            codecAlive = false
            timing.retire(timingEpoch)
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

    fun resetCodec() {
        synchronized(monitor) {
            // Every replacement decoder needs headers and a keyframe. Leave
            // creation to reconnect, which obtains both from the host.
            events.invalidated()
            releaseCodec()
        }
    }

}
