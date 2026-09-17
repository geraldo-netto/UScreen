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
    var inputClock: () -> Long = System::nanoTime
    private val renderedCount = AtomicLong(0)
    private val outputWatchdog = DecoderOutputWatchdog()
    private var timingEpoch = timing.currentEpoch()
    private var lifetime: CodecLifetime? = null
    private var retiring: CodecLifetime? = null
    private var callbackDecoder: CallbackDecoder? = null
    var profile = DecoderProfile()

    fun setupCodec(surface: Surface, parameters: DecoderFormat): Boolean {
        synchronized(monitor) {
            // Bound retired ownership on reconnect and across Activity/receiver
            // recreation. A stuck native call must not admit repeated codecs.
            if (retiring?.finished == false || CodecLifetime.retirementPending()) return false
            retiring = null
            val codecTiming = timing.beginEpoch()
            var pendingCodec: MediaCodec? = null
            var pendingThread: HandlerThread? = null
            var pendingOwner: CodecLifetime? = null
            var pendingCallbacks: CallbackDecoder? = null
            try {
                outputWatchdog.restarted(outputClock())

                val codec = createCodec(parameters.mimeType)
                pendingCodec = codec
                val owner = CodecLifetime(codec)
                pendingOwner = owner
                val format = DecoderConfiguration.format(codec, parameters, profile, outputWatchdog.lowLatencyHints)

                // Acknowledge MediaCodec's render notification. Callback delivery can
                // be delayed or batched, so its execution is not a physical-screen
                // timestamp. The host sequence travels as the presentation timestamp;
                // the separate MediaCodec render-time argument is currently unused.
                val cbThread = callbackThreadFactory()
                pendingThread = cbThread
                cbThread.start()
                val handler = Handler(cbThread.looper)
                val callbacks = callbacks(owner, handler, codecTiming)
                pendingCallbacks = callbacks
                if (callbacks != null) codec.setCallback(callbacks, handler)
                codec.configure(format, surface, null, 0)
                codec.setVideoScalingMode(MediaCodec.VIDEO_SCALING_MODE_SCALE_TO_FIT)
                codec.setOnFrameRenderedListener({ _, presentationTimeUs, _ ->
                    notifyRendered(codec, codecTiming, presentationTimeUs.toInt())
                }, handler)

                codec.start()
                timingEpoch = codecTiming
                mediaCodec = codec
                lifetime = owner
                callbackDecoder = callbacks
                frameCallbackThread = cbThread
                codecAlive = true
                if (callbacks == null) startOutputThread(codec, codecTiming)
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
                    lifetime = null
                }
                if (frameCallbackThread === pendingThread) frameCallbackThread = null
                pendingCallbacks?.close()
                pendingOwner?.let { retiring = it; it.retire(); it.awaitRetirement(500) }
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

    private fun callbacks(owner: CodecLifetime, handler: Handler, epoch: FrameTiming.Epoch): CallbackDecoder? {
        if (!profile.callbacks) return null
        val capturedStatistics = statistics()
        return CallbackDecoder(owner, handler,
            queued = { synchronized(monitor) { if (mediaCodec === owner.codec) checkOutputProgress() } },
            output = { recordOutput(owner.codec, epoch, capturedStatistics, it) },
            failed = { retireFailedOutput(owner.codec, "Decoder callback failed", it) }, clock = inputClock)
    }

    private fun recordOutput(codec: MediaCodec, epoch: FrameTiming.Epoch, statistics: ReceiverStatistics, sequence: Int) {
        timing.noteReleased(sequence, epoch)
        val now = outputClock()
        synchronized(monitor) {
            if (mediaCodec !== codec || !codecAlive) return
            outputWatchdog.output(now)
            statistics.frameRendered()
        }
    }

    /**
     * Dedicated render thread: drains decoded frames and releases them to the
     * surface as soon as they're ready, independent of network reads. This
     * reduces coupling to network stalls; it does not impose a latency bound.
     */
    fun startOutputThread(codec: MediaCodec, codecTiming: FrameTiming.Epoch = timingEpoch) {
        val outputStatistics = statistics()
        val outputOwner = synchronized(monitor) { ownerFor(codec) } ?: return
        outputThread = Thread({
            val info = MediaCodec.BufferInfo()
            var rendered = 0L
            while (codecAlive && mediaCodec === codec) {
                try {
                    val index = outputOwner.use { codec.dequeueOutputBuffer(info, 10_000) } ?: break
                    if (mediaCodec !== codec || !codecAlive || outputOwner.retired) break
                    if (index >= 0) {
                        val seq = info.presentationTimeUs.toInt()
                        outputOwner.use { codec.releaseOutputBuffer(index, true) } ?: break
                        recordOutput(codec, codecTiming, outputStatistics, seq)
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
            priority = profile.renderPriority
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
        val (inputOwner, callbacks) = synchronized(monitor) {
            val owner = ownerFor(codec) ?: return
            if (!isConfig) timing.noteArrival(presentationTimeUs.toInt(), timingEpoch, arrivalNanos)
            owner to callbackDecoder
        }
        try {
            val input = DecoderInput(data, offset, size, isConfig, presentationTimeUs)
            if (callbacks != null) {
                check(callbacks.offer(input) || inputOwner.retired) { "Decoder callback input queue stalled" }
                return
            }
            if (queueInput(inputOwner, input) && !isConfig) {
                synchronized(monitor) {
                    if (mediaCodec === codec) checkOutputProgress()
                }
            }
        } catch (e: Exception) {
            retireFailedOutput(codec, "Decoder feed error", e)
        }
    }

    private fun queueInput(owner: CodecLifetime, input: DecoderInput): Boolean {
        repeat(10) {
            val queued = owner.use {
                val index = owner.codec.dequeueInputBuffer(20_000)
                if (index < 0 || owner.retired) false
                else { input.write(owner.codec, index); true }
            } ?: return false
            if (queued) return true
            if (owner.retired) return false
        }
        throw IllegalStateException("Decoder input unavailable after ten 20ms waits")
    }

    // Caller holds the receiver monitor. The lazy branch supports a codec
    // adopted by a platform/test adapter before starting its I/O workers.
    private fun ownerFor(codec: MediaCodec): CodecLifetime? {
        if (mediaCodec !== codec) return null
        if (lifetime?.codec !== codec) lifetime = CodecLifetime(codec)
        return lifetime?.takeUnless { it.retired }
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

    fun retireOutput() { codecAlive = false; lifetime?.closeAdmission(); callbackDecoder?.close() }

    /** Tear the decoder down without touching the surface or the socket. */
    fun releaseCodec() {
        synchronized(monitor) {
            codecAlive = false
            timing.retire(timingEpoch)
            val owner = mediaCodec?.let { ownerFor(it) } ?: lifetime
            mediaCodec = null
            lifetime = null
            callbackDecoder?.close()
            callbackDecoder = null
            outputThread = null
            frameCallbackThread?.quitSafely()
            frameCallbackThread = null
            if (owner != null) {
                retiring = owner
                owner.retire()
                owner.awaitRetirement(500)
            }
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
