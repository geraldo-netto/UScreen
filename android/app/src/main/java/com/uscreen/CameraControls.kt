package com.uscreen

import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp

@Composable
internal fun CameraControls(binding: CameraBinding) {
    Text("Camera sharing", style = MaterialTheme.typography.titleMedium)
    Text(binding.status, style = MaterialTheme.typography.bodySmall)
    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        FilterChip(selected = binding.selected == null, onClick = { binding.choose(null) }, label = { Text("Off") })
        CameraLens.values().forEach { lens ->
            FilterChip(selected = binding.selected == lens, enabled = binding.endpoint != null,
                onClick = { binding.choose(lens) }, label = { Text(lens.label) })
        }
    }
    Text("Select the matching UScreen webcam in your video call. Only one camera is live. Leaving UScreen stops sharing.",
        style = MaterialTheme.typography.bodySmall)
    Spacer(Modifier.height(20.dp))
}
