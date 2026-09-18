package com.uscreen.benchmark

import android.media.MediaCodec
import android.media.MediaCodecInfo
import android.os.Build
import android.view.Surface
import com.uscreen.*
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.locks.LockSupport
import org.json.JSONObject

internal class DecoderReplay(private val surface: Surface, private val profile: String, private val active: AtomicBoolean) {
    private val stats = ReplayStats()
    private val timings = FrameTiming()
    private val result = JSONObject()
    private val decoder = DecoderSession(Any(), active::get, timings, object : DecoderEvents {
        override fun rendered(sequence: Int, decodeMicros: Int) {
            BenchMetrics.acknowledged(sequence)
            stats.rendered(sequence, decodeMicros)
        }
        override fun invalidated() { stats.invalidated(); active.set(false) }
    }, { ReceiverStatistics() })
    private var sequence = 1L
    private var input: ReplayInput? = null

    fun run(clip: ReplayClip, rate: Int, seconds: Int, warmup: Int): JSONObject {
        try {
            configure(clip)
            feed(clip.config, true)
            phase(clip, rate, warmup)
            result.put("before", ReplayStats.process()).put("dequeues_before", BenchMetrics.snapshot())
            val first = sequence.toInt()
            stats.begin(first)
            val sent = phase(clip, rate, seconds)
            result.put("after", ReplayStats.process()).put("dequeues_after", BenchMetrics.snapshot())
            val drain = System.nanoTime()
            while (active.get() && stats.count() < sent && System.nanoTime() - drain < 750_000_000) {
                LockSupport.parkNanos(1_000_000)
            }
            result.put("callback_drain_ns", System.nanoTime() - drain)
            result.put("trace_columns", org.json.JSONArray(listOf("sequence", "feed_ns", "release_note_ns",
                "notification_ns", "reported_render_ns", "ack_event_ns")))
            result.put("trace", BenchMetrics.trace(first, sequence.toInt()))
            input?.let { result.put("input_transport", it.summary()) }
            return result.put("stats", stats.finish()).put("sent", sent).put("completed", active.get())
                .put("profile", profile).put("seconds", seconds).put("warmup", warmup).put("send_fps", rate)
                .put("width", clip.width).put("height", clip.height).put("stream_fps", clip.fps)
                .put("fixture_sha256", clip.sha256).put("fingerprint", Build.FINGERPRINT).put("sdk", Build.VERSION.SDK_INT)
        } finally { decoder.releaseCodec(); input?.close() }
    }

    private fun configure(clip: ReplayClip) {
        applyProfile(decoder, profile) // Generated bridge: legacy source lacks profile selection.
        decoder.createCodec = { mime -> MediaCodec.createDecoderByType(mime).also { result.put("codec", inventory(it, mime, clip)) } }
        check(decoder.setupCodec(surface, DecoderFormat("video/avc", clip.width, clip.height, clip.fps)))
        input = createReplayInput(decoder, profile, active::get)
    }

    private fun inventory(codec: MediaCodec, mime: String, clip: ReplayClip): JSONObject {
        val info = codec.codecInfo
        val caps = info.getCapabilitiesForType(mime)
        val row = JSONObject().put("name", info.name).put("instances", caps.maxSupportedInstances)
            .put("supports_2x", caps.videoCapabilities.areSizeAndRateSupported(clip.width, clip.height, clip.fps * 2.0))
        if (Build.VERSION.SDK_INT >= 29) row.put("hardware", info.isHardwareAccelerated).put("software", info.isSoftwareOnly)
        if (Build.VERSION.SDK_INT >= 30) row.put("low_latency", caps.isFeatureSupported(MediaCodecInfo.CodecCapabilities.FEATURE_LowLatency))
        return row
    }

    private fun phase(clip: ReplayClip, rate: Int, seconds: Int): Int {
        val start = System.nanoTime()
        var sent = 0
        repeat(rate * seconds) { index ->
            val deadline = start + index * 1_000_000_000L / rate
            parkUntil(deadline)
            if (!active.get()) return sent
            feed(clip.frames[index % clip.frames.size], false)
            sent++
        }
        parkUntil(start + seconds * 1_000_000_000L)
        return sent
    }
    private fun parkUntil(deadline: Long) {
        while (active.get()) {
            val remaining = deadline - System.nanoTime()
            if (remaining <= 0) return
            LockSupport.parkNanos(remaining.coerceAtMost(50_000_000))
        }
    }
    private fun feed(data: ByteArray, configuration: Boolean) {
        val codec = decoder.mediaCodec ?: error("Decoder retired")
        if (!configuration) BenchMetrics.arrived(sequence.toInt())
        val number = if (configuration) 0 else sequence++
        val transport = input
        if (transport == null) decoder.feedDecoder(codec, data, 0, data.size, configuration, number)
        else transport.feed(data, configuration, number)
    }
}
