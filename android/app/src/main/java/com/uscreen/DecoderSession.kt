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

internal data class DecoderFormat(val mimeType: String, val width: Int, val height: Int, val fps: Int, val codecPrivate: ByteArray? = null)

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
    var createCodec: (DecoderFormat) -> MediaCodec = DecoderConfiguration::create
    var outputClock: () -> Long = System::nanoTime
    var inputClock: () -> Long = System::nanoTime
    private val renderedCount = AtomicLong(0)
    private val discardedCount = AtomicLong(0)
    val discardedOutputs: Long get() = discardedCount.get()
    private val outputWatchdog = DecoderOutputWatchdog()
    private var timingEpoch = timing.currentEpoch()
    private var lifetime: CodecLifetime? = null
    private var retiring: CodecLifetime? = null
    private var callbackDecoder: CallbackDecoder? = null
    var profile = DecoderProfile()

    private class Startup(val epoch: FrameTiming.Epoch, val profile: DecoderProfile,
                          val hints: Boolean, val valid: () -> Boolean) {
        var owner: CodecLifetime? = null
        var thread: HandlerThread? = null
        var callbacks: CallbackDecoder? = null
    }
    private var startup: Startup? = null // guarded by monitor

    /** Called on the receiver's I/O worker. Native setup never owns monitor. */
    fun setupCodec(surface: Surface, parameters: DecoderFormat, valid: () -> Boolean = { true }): Boolean {
        val attempt = reserveStartup(valid) ?: return false
        var published = false
        try {
            val codec = createCodec(parameters)
            attempt.owner = CodecLifetime(codec)
            if (!startupCurrent(attempt)) return false
            configureStartup(attempt, codec, surface, parameters)
            if (!startupCurrent(attempt)) return false
            codec.start()
            published = publishStartup(attempt)
            return published
        } catch (error: Exception) {
            Log.e(TAG, "Failed to setup codec", error)
            return false
        } finally {
            if (!published) discardStartup(attempt)
            CodecLifetime.finishStartup()
        }
    }

    private fun reserveStartup(valid: () -> Boolean): Startup? = synchronized(monitor) {
        if (!valid() || startup != null || mediaCodec != null || retiring?.finished == false) return null
        if (!CodecLifetime.beginStartup()) return null
        retiring = null
        outputWatchdog.restarted(outputClock())
        Startup(timing.beginEpoch(), profile, outputWatchdog.lowLatencyHints, valid).also { startup = it }
    }

    private fun startupCurrent(attempt: Startup): Boolean = synchronized(monitor) {
        startup === attempt && attempt.valid()
    }

    private fun configureStartup(attempt: Startup, codec: MediaCodec, surface: Surface, parameters: DecoderFormat) {
        val format = DecoderConfiguration.format(codec, parameters, attempt.profile, attempt.hints)
        if (!startupCurrent(attempt)) return
        val thread = callbackThreadFactory().also { attempt.thread = it; it.start() }
        val handler = Handler(thread.looper)
        val callbacks = callbacks(attempt.owner!!, handler, attempt.epoch, attempt.profile)
        attempt.callbacks = callbacks
        if (callbacks != null) codec.setCallback(callbacks, handler)
        codec.configure(format, surface, null, 0)
        if (!startupCurrent(attempt)) return
        codec.setVideoScalingMode(MediaCodec.VIDEO_SCALING_MODE_SCALE_TO_FIT)
        // Callback execution is acknowledgement timing, not physical presentation.
        codec.setOnFrameRenderedListener({ _, presentationTimeUs, _ ->
            notifyRendered(codec, attempt.epoch, presentationTimeUs.toInt())
        }, handler)
    }

    private fun publishStartup(attempt: Startup): Boolean = synchronized(monitor) {
        if (!startupCurrent(attempt)) return false
        val owner = attempt.owner!!
        discardedCount.set(0)
        timingEpoch = attempt.epoch
        mediaCodec = owner.codec
        lifetime = owner
        callbackDecoder = attempt.callbacks
        frameCallbackThread = attempt.thread
        codecAlive = true
        if (attempt.callbacks == null) startOutputThread(owner.codec, attempt.epoch)
        startup = null
        Log.i(TAG, "Codec configured and started with surface")
        true
    }

    private fun discardStartup(attempt: Startup) {
        synchronized(monitor) {
            if (startup === attempt) startup = null
            timing.retire(attempt.epoch)
            if (mediaCodec === attempt.owner?.codec) {
                codecAlive = false
                mediaCodec = null
                lifetime = null
                callbackDecoder = null
                frameCallbackThread = null
            }
        }
        attempt.callbacks?.close()
        attempt.thread?.let {
            it.quitSafely()
            if (Thread.currentThread() !== it) it.join(500)
        }
        // Setup owns these locals until its native call returns. Retirement can
        // now free them; the process-wide gate remains closed until then.
        attempt.owner?.let { it.retire(); it.awaitRetirement(500) }
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

    private fun callbacks(owner: CodecLifetime, handler: Handler, epoch: FrameTiming.Epoch, selected: DecoderProfile): CallbackDecoder? {
        if (!selected.callbacks) return null
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
            val drainer = outputDrainer(codec, outputOwner)
            val latest = profile.renderLatest
            var rendered = 0L
            while (codecAlive && mediaCodec === codec) {
                try {
                    val index = outputOwner.use { codec.dequeueOutputBuffer(info, 10_000) } ?: break
                    if (mediaCodec !== codec || !codecAlive || outputOwner.retired) break
                    if (index >= 0) {
                        val seq = outputOwner.use {
                            drainer.release(index, info.presentationTimeUs.toInt(), latest)
                        } ?: break
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

    private fun outputDrainer(codec: MediaCodec, owner: CodecLifetime) = DecodedOutputDrainer(codec,
        { mediaCodec === codec && codecAlive && !owner.retired },
        { discardedCount.incrementAndGet() })

    private fun retireFailedOutput(codec: MediaCodec, message: String, error: Exception) {
        if (codecAlive) Log.w(TAG, message, error)
        synchronized(monitor) {
            if (mediaCodec === codec || startup?.owner?.codec === codec) resetCodec()
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
            if (queueInput(inputOwner) { slot -> input.write(codec, slot) } && !isConfig) {
                synchronized(monitor) {
                    if (mediaCodec === codec) checkOutputProgress()
                }
            }
        } catch (e: Exception) {
            retireFailedOutput(codec, "Decoder feed error", e)
        }
    }

    /** T403 experiment: a transport read borrows codec storage, outside the
     * receiver monitor. Caller must close its reader when the session retires. */
    fun feedDirect(codec: MediaCodec, info: DecoderInputInfo, fill: (java.nio.ByteBuffer) -> Unit): Boolean {
        val (owner, epoch) = synchronized(monitor) {
            if (callbackDecoder != null) return false
            (ownerFor(codec) ?: return false) to timingEpoch
        }
        return try {
            val queued = queueInput(owner) { index ->
                queueCodecInput(codec, index, info) { buffer ->
                    fill(buffer)
                    check(!owner.retired) { "Codec retired during input read" }
                    if (!info.configuration) timing.noteArrival(info.sequence.toInt(), epoch, System.nanoTime())
                }
            }
            if (queued && !info.configuration) synchronized(monitor) {
                if (mediaCodec === codec) checkOutputProgress()
            }
            queued
        } catch (error: Exception) {
            retireFailedOutput(codec, "Direct decoder input failed", error)
            false
        }
    }

    private fun queueInput(owner: CodecLifetime, submit: (Int) -> Unit): Boolean {
        repeat(10) {
            val queued = owner.use {
                val index = owner.codec.dequeueInputBuffer(20_000)
                if (index < 0 || owner.retired) false
                else { submit(index); true }
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
            startup?.let { timing.retire(it.epoch) }
            startup = null
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
