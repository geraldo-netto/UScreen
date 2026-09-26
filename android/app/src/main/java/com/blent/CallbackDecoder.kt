package com.blent

import android.media.MediaCodec
import android.media.MediaFormat
import android.os.Handler
import android.os.Looper
import java.util.ArrayDeque
import java.util.concurrent.atomic.AtomicBoolean

/** All callback-mode input/output operations run on this handler. Neither the
 * network producer nor codec retirement waits on it while holding codec state. */
internal class CallbackDecoder(
    private val owner: CodecLifetime,
    private val handler: Handler,
    private val queued: () -> Unit,
    private val output: (Int) -> Unit,
    private val failed: (Exception) -> Unit,
    private val clock: () -> Long = System::nanoTime,
) : MediaCodec.Callback() {
    private val mailbox = DecoderMailbox(clock)
    private val indices = ArrayDeque<Int>() // Confined to handler.
    private val wakePosted = AtomicBoolean(false)
    private val timeout = Runnable { pump() }
    private val wake = Runnable { wakePosted.set(false); pump() }

    fun offer(input: DecoderInput): Boolean {
        if (!mailbox.offer(input)) return false
        if (wakePosted.compareAndSet(false, true) && !handler.post(wake)) {
            close()
            return false
        }
        return true
    }

    fun close() {
        mailbox.close()
        handler.removeCallbacks(wake)
        handler.removeCallbacks(timeout)
    }

    private fun dispatch(codec: MediaCodec, operation: () -> Unit) {
        if (codec !== owner.codec || mailbox.closed || owner.retired) return
        val guarded = Runnable {
            if (!mailbox.closed && !owner.retired) {
                try { operation() } catch (error: Exception) { fail(error) }
            }
        }
        if (Looper.myLooper() === handler.looper) guarded.run() else handler.post(guarded)
    }

    override fun onInputBufferAvailable(codec: MediaCodec, index: Int) = dispatch(codec) {
        check(index >= 0 && indices.size < 64 && !indices.contains(index)) { "Invalid/repeated codec input slot" }
        indices.addLast(index)
        pump()
    }

    override fun onOutputBufferAvailable(codec: MediaCodec, index: Int, info: MediaCodec.BufferInfo) {
        val sequence = info.presentationTimeUs.toInt()
        dispatch(codec) {
            owner.use { codec.releaseOutputBuffer(index, true) } ?: return@dispatch
            output(sequence)
        }
    }

    override fun onError(codec: MediaCodec, error: MediaCodec.CodecException) = dispatch(codec) { fail(error) }
    override fun onOutputFormatChanged(codec: MediaCodec, format: MediaFormat) {}

    private fun pump() {
        handler.removeCallbacks(timeout)
        if (mailbox.closed || owner.retired) return
        try {
            while (indices.isNotEmpty()) {
                val next = mailbox.first() ?: return
                submit(next)
                if (mailbox.closed || owner.retired) return
            }
            scheduleTimeout()
        } catch (error: Exception) { fail(error) }
    }

    private fun submit(next: DecoderMailbox.Pending) {
        check(clock() < next.deadline) { "Decoder input unavailable for 200ms" }
        val index = indices.removeFirst()
        owner.use { next.input.write(owner.codec, index) } ?: return
        mailbox.complete(next)
        if (!next.input.configuration) queued()
    }

    private fun scheduleTimeout() {
        val next = mailbox.first() ?: return
        val remaining = next.deadline - clock()
        check(remaining > 0) { "Decoder input unavailable for 200ms" }
        handler.postDelayed(timeout, (remaining + 999_999) / 1_000_000)
    }

    private fun fail(error: Exception) {
        close()
        failed(error)
    }
}
