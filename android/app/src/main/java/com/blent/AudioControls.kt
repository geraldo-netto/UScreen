package com.blent

import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.contentDescription
import kotlin.math.roundToInt

@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun AudioControls(binding: AudioBinding) {
    SettingsSection("Microphone sharing")
    Text(binding.status, style = MaterialTheme.typography.bodySmall)
    Text("Computer Start requests a session. Changing tablet settings stops it.", style = MaterialTheme.typography.bodySmall)
    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        Button(onClick = binding::accept, enabled = binding.pending != null) { Text("Allow microphone") }
        OutlinedButton(onClick = binding::stopSharing, enabled = binding.sharing || binding.pending != null) { Text("Stop microphone") }
    }
    AudioProcessing(binding)
    Row {
        Checkbox(binding.preferences.background, { binding.configure(binding.preferences.copy(background = it)) },
            modifier = Modifier.semantics { contentDescription = "Allow background microphone" })
        Text("Allow background microphone (notification includes Stop)")
    }
    Row {
        Checkbox(binding.preferences.builtIn, { binding.configure(binding.preferences.copy(builtIn = it)) },
            modifier = Modifier.semantics { contentDescription = "Use built-in microphone" })
        Text("Use built-in microphone")
    }
    Text("Microphone gain: ${binding.preferences.gain}%")
    Slider(binding.preferences.gain.toFloat(), { binding.configure(binding.preferences.copy(gain = it.roundToInt())) }, valueRange = 0f..200f)
    Text("Raw capture requires native support and can contain speaker echo. Speech requests AEC; availability does not prove echo removal.", style = MaterialTheme.typography.bodySmall)
    Spacer(Modifier.height(20.dp))
}
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun AudioProcessing(binding: AudioBinding) {
    Text("Processing")
    FlowRow(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
        listOf("Use host setting", "Speech", "Raw").forEachIndexed { value, label ->
            FilterChip(selected = binding.preferences.processing == value,
                onClick = { binding.configure(binding.preferences.copy(processing = value)) }, label = { Text(label) })
        }
    }
}
