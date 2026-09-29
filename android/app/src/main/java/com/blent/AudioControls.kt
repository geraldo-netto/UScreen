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
    SettingsSection("${binding.label.replaceFirstChar { it.uppercase() }} sharing")
    Text(binding.status, style = MaterialTheme.typography.bodySmall)
    Text("Computer Start requests a session. Changing tablet settings stops it.", style = MaterialTheme.typography.bodySmall)
    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        Button(onClick = binding::accept, enabled = binding.pending != null) { Text("Allow ${binding.label}") }
        OutlinedButton(onClick = binding::stopSharing, enabled = binding.sharing || binding.pending != null) { Text("Stop ${binding.label}") }
    }
    AudioProcessing(binding)
    Row {
        Checkbox(binding.preferences.background, { binding.configure(binding.preferences.copy(background = it)) },
            modifier = Modifier.semantics { contentDescription = "Allow background ${binding.label}" })
        Text("Allow background ${binding.label} (notification includes Stop)")
    }
    Row {
        Checkbox(binding.preferences.builtIn, { binding.configure(binding.preferences.copy(builtIn = it)) },
            modifier = Modifier.semantics { contentDescription = "Use built-in ${binding.label}" })
        Text("Use built-in ${binding.label}")
    }
    Text("${if (binding.direction == 1) "Microphone gain" else "Speaker volume"}: ${binding.preferences.gain}%")
    Slider(binding.preferences.gain.toFloat(), { binding.configure(binding.preferences.copy(gain = it.roundToInt())) }, valueRange = 0f..(if (binding.direction == 1) 200f else 100f))
    Text(if (binding.direction == 1) "Raw capture requires native support and can contain speaker echo. Speech requests AEC; availability does not prove echo removal."
        else "Speech uses communication playback; Raw uses media playback. Focus loss pauses or stops this session. System volume stays unchanged.", style = MaterialTheme.typography.bodySmall)
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
