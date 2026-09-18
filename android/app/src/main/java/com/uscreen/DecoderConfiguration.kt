package com.uscreen

import android.media.MediaCodec
import android.media.MediaCodecInfo
import android.media.MediaFormat
import android.os.Build

internal enum class DecoderHints { LEGACY, SUPPORTED, NONE }

/** Experimental profiles remain separate from the compatibility default until
 * device measurements justify selection. Watchdog fallback overrides hints. */
internal data class DecoderProfile(
    val callbacks: Boolean = false,
    val hints: DecoderHints = DecoderHints.LEGACY,
    val operatingRateMultiplier: Int? = 2,
    val renderPriority: Int = Thread.MAX_PRIORITY,
    val renderLatest: Boolean = false,
)

internal object DecoderConfiguration {
    fun create(parameters: DecoderFormat): MediaCodec {
        val format = MediaFormat.createVideoFormat(parameters.mimeType, parameters.width, parameters.height)
        format.setInteger(MediaFormat.KEY_FRAME_RATE, parameters.fps)
        val name = checkNotNull(DecoderCapabilities.decoderName(format, parameters.mimeType)) {
            "No compatible decoder for stream format"
        }
        return try { MediaCodec.createByCodecName(name) }
        catch (failure: Exception) { throw IllegalStateException("Unable to create compatible decoder $name", failure) }
    }

    internal interface Support {
        fun lowLatency(codec: MediaCodec, mime: String): Boolean
        fun operatingRate(codec: MediaCodec, parameters: DecoderFormat, fps: Int): Boolean
    }
    fun format(codec: MediaCodec, parameters: DecoderFormat, profile: DecoderProfile, allowHints: Boolean,
               support: Support = PlatformSupport): MediaFormat {
        val format = MediaFormat.createVideoFormat(parameters.mimeType, parameters.width, parameters.height)
        parameters.codecPrivate?.takeIf { it.isNotEmpty() }?.let {
            format.setByteBuffer("csd-0", java.nio.ByteBuffer.wrap(it))
        }
        format.setInteger(MediaFormat.KEY_FRAME_RATE, parameters.fps)
        format.setInteger(MediaFormat.KEY_I_FRAME_INTERVAL, 1)
        format.setInteger(MediaFormat.KEY_COLOR_RANGE, MediaFormat.COLOR_RANGE_LIMITED)
        format.setInteger(MediaFormat.KEY_COLOR_STANDARD, MediaFormat.COLOR_STANDARD_BT709)
        format.setInteger(MediaFormat.KEY_COLOR_TRANSFER, MediaFormat.COLOR_TRANSFER_SDR_VIDEO)
        if (allowHints && profile.hints != DecoderHints.NONE) hints(codec, parameters, profile, format, support)
        return format
    }

    private fun hints(codec: MediaCodec, parameters: DecoderFormat, profile: DecoderProfile, format: MediaFormat, support: Support) {
        val legacy = profile.hints == DecoderHints.LEGACY
        if (Build.VERSION.SDK_INT >= 30 && (legacy || support.lowLatency(codec, parameters.mimeType))) {
            format.setInteger(MediaFormat.KEY_LOW_LATENCY, 1)
        }
        val rate = profile.operatingRateMultiplier?.times(parameters.fps)
        if (rate != null && (legacy || support.operatingRate(codec, parameters, rate))) {
            format.setInteger(MediaFormat.KEY_OPERATING_RATE, rate)
        }
        // Retain the existing compatibility profile verbatim. Experiments do
        // not apply Qualcomm's private key to unrelated decoder vendors.
        if (legacy) format.setInteger("vendor.qti-ext-dec-low-latency.enable", 1)
    }

    private object PlatformSupport : Support {
        override fun lowLatency(codec: MediaCodec, mime: String): Boolean = try {
            Build.VERSION.SDK_INT >= 30 && codec.codecInfo.getCapabilitiesForType(mime)
                .isFeatureSupported(MediaCodecInfo.CodecCapabilities.FEATURE_LowLatency)
        } catch (_: Exception) { false }

        override fun operatingRate(codec: MediaCodec, parameters: DecoderFormat, fps: Int): Boolean = try {
            codec.codecInfo.getCapabilitiesForType(parameters.mimeType).videoCapabilities
                ?.areSizeAndRateSupported(parameters.width, parameters.height, fps.toDouble()) == true
        } catch (_: Exception) { false }
    }
}
