package com.uscreen

import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlin.math.roundToInt

/** Tablet display controls apply immediately, independently of stream settings. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun DisplayControls(settings: SettingsValues, refreshRates: List<Float>, onEvent: (SettingsEvent) -> Unit) {
    val brightness = settings.brightness
    val refreshRate = settings.refreshRate
    val rates = (refreshRates + Prefs.DEFAULT_DISPLAY_REFRESH_RATE + refreshRate)
        .filter { it.isFinite() && it > 0f }.distinct().sorted()
    val accent = MaterialTheme.colorScheme.primary

    Text("Brightness: $brightness%", fontSize = 14.sp, color = Color(0xFFB0B0C0))
    Slider(
        value = brightness.toFloat(),
        onValueChange = {
            onEvent(SettingsEvent.Brightness(it.roundToInt()))
        },
        valueRange = 0f..100f,
        modifier = Modifier.semantics { contentDescription = "Brightness" },
        colors = SliderDefaults.colors(thumbColor = accent, activeTrackColor = accent)
    )
    Text("Display refresh rate", fontSize = 14.sp, color = Color(0xFFB0B0C0))
    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        (listOf(0f) + rates).forEach { rate ->
            FilterChip(
                selected = refreshRate == rate,
                onClick = {
                    onEvent(SettingsEvent.RefreshRate(rate))
                },
                label = { Text(if (rate == 0f) "System default" else "${formatRefreshRate(rate)} Hz") },
                colors = FilterChipDefaults.filterChipColors(
                    selectedContainerColor = accent, selectedLabelColor = Color.White)
            )
        }
    }
    Text("Applies immediately in UScreen only. The closest supported refresh rate is used.",
        fontSize = 11.sp, color = Color(0xFF6A6A7E))
}

private fun formatRefreshRate(rate: Float): String =
    if (rate == rate.roundToInt().toFloat()) rate.roundToInt().toString() else rate.toString()
