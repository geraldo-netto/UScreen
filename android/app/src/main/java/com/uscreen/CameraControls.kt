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
    Text("Configure cameras in the Cameras tab of UScreen on your computer.",
        style = MaterialTheme.typography.bodySmall)
    Spacer(Modifier.height(20.dp))
}
