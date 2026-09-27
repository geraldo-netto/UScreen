// Copyright (c) 2026 Geraldo Netto
package com.blent

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable

@Composable
internal fun DecoderDiagnosticsContent(state: DecoderDiagnostics) {
    SettingsSection("Decoder diagnostics")
    val active = state.active
    if (active == null) Text("No active decoder") else ActiveDecoderDetails(active)
    if (state.watchdogFallback) Text("Watchdog fallback: performance hints disabled after repeated output stalls.")
    Text("Advertised support is not measured speed. Effective hints are unknown; Android may ignore requests.",
        style = MaterialTheme.typography.bodySmall)
}

@Composable
private fun ActiveDecoderDetails(active: ActiveDecoderDiagnostics) {
    Text("Active codec: ${active.name ?: "Unknown"}")
    Text("Stream: ${active.mime}, ${active.width} × ${active.height}, ${active.fps} FPS")
    Text("Hardware accelerated: ${supportLabel(active.hardware)}")
    Text("Advertised standard low latency: ${supportLabel(active.lowLatency)}")
    Text("Advertised 2× rate headroom at this size: ${supportLabel(active.doubleRate)}")
    Text("Requested settings: ${active.requested.joinToString(", ").ifEmpty { "None" }}")
}

internal fun supportLabel(value: Boolean?): String = when (value) {
    true -> "Yes"
    false -> "No"
    null -> "Unknown"
}
