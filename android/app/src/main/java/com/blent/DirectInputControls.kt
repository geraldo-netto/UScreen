package com.blent

import androidx.compose.foundation.layout.Row
import androidx.compose.material3.Button
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue

/** Controls stay on the tablet; finger positions still refer to the full surface. */
@Composable
internal fun DirectInputControls(capture: TouchCapture?) {
    if (capture == null) return
    val state by capture.motion.direct.state.collectAsState()
    val value = state ?: return
    Row {
        Button(onClick = { capture.selectDirect("touch") }, enabled = value.ready && value.touch) { Text("Touch") }
        Button(onClick = { capture.selectDirect("direct_mouse") }, enabled = value.ready && value.mouse) { Text("Mouse") }
        if (value.mode == "direct_mouse") {
            Button(onClick = capture::rightClick, enabled = value.ready) { Text("Right-click") }
            Button(onClick = capture::toggleDrag, enabled = value.ready) { Text(if (value.drag) "Cancel drag" else "Drag next gesture") }
        }
    }
}
