// Copyright (c) 2026 Geraldo Netto
package com.blent

import android.media.MediaCodec
import android.media.MediaCodecInfo
import android.media.MediaFormat
import android.os.Build

/** Published only after the owning decoder starts; no effective-hint claim. */
internal data class ActiveDecoderDiagnostics(
    val name: String?, val mime: String, val width: Int, val height: Int, val fps: Int,
    val hardware: Boolean?, val lowLatency: Boolean?, val doubleRate: Boolean?,
    val requested: List<String>,
)
internal data class DecoderDiagnostics(
    val active: ActiveDecoderDiagnostics? = null,
    val watchdogFallback: Boolean = false,
)

/** Native queries stay on the decoder setup worker, outside the UI/owner lock.
 * Optional query failures cannot make a compatible decoder fail to start. */
internal object DecoderDiagnosticProbe {
    fun capture(codec: MediaCodec, parameters: DecoderFormat, configured: MediaFormat): ActiveDecoderDiagnostics {
        val info = observe { codec.codecInfo }
        val caps = observe { info?.getCapabilitiesForType(parameters.mimeType) }
        return ActiveDecoderDiagnostics(
            observe { codec.name }, parameters.mimeType, parameters.width, parameters.height, parameters.fps,
            observe { if (Build.VERSION.SDK_INT >= 29) info?.isHardwareAccelerated else null },
            observe { if (Build.VERSION.SDK_INT >= 30) caps?.isFeatureSupported(MediaCodecInfo.CodecCapabilities.FEATURE_LowLatency) else null },
            doubleRate(caps, parameters), requested(configured),
        )
    }

    internal fun doubleRate(caps: MediaCodecInfo.CodecCapabilities?, parameters: DecoderFormat): Boolean? {
        if (parameters.width <= 0 || parameters.height <= 0 || parameters.fps !in 1..Int.MAX_VALUE / 2) return null
        return observe { caps?.videoCapabilities?.areSizeAndRateSupported(
            parameters.width, parameters.height, parameters.fps.toDouble() * 2.0,
        ) }
    }

    internal fun requested(format: MediaFormat): List<String> = listOf(
        "low-latency" to "Standard low latency",
        MediaFormat.KEY_OPERATING_RATE to "Operating rate",
        "vendor.qti-ext-dec-low-latency.enable" to "Vendor low latency",
    ).mapNotNull { (key, label) ->
        if (!format.containsKey(key)) null
        else "$label=${observe { format.getInteger(key) } ?: "unknown"}"
    }

    private fun <T> observe(query: () -> T?): T? = try { query() } catch (_: Exception) { null }
}
