package com.uscreen

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlin.math.roundToInt

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun SettingsSheet(
    settings: SettingsValues,
    updateAvailable: String?,
    onOpenUpdate: () -> Unit,
    penOnly: Boolean,
    onDismiss: () -> Unit,
    displayRefreshRates: List<Float> = listOf(Prefs.DEFAULT_DISPLAY_REFRESH_RATE),
    onSettingsEvent: (SettingsEvent) -> Unit = {},
) {
    var bitrateMbps by remember(settings.bitrateKbps) {
        mutableStateOf(settings.bitrateKbps / 1000f)
    }
    var fpsChoice by remember(settings.fps) { mutableStateOf(settings.fps) }

    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
        containerColor = Color(0xFF16161F)
    ) {
        Column(modifier = Modifier.verticalScroll(rememberScrollState()).padding(horizontal = 24.dp, vertical = 8.dp)) {
            Text(
                "Settings",
                fontSize = 20.sp,
                fontWeight = FontWeight.Bold,
                color = Color.White
            )
            Spacer(Modifier.height(20.dp))

            UpdateNotice(updateAvailable, onOpenUpdate)
            settings.streamError?.let { Text("Settings rejected: $it", color = MaterialTheme.colorScheme.error) }

            DisplayControls(settings, displayRefreshRates, onSettingsEvent)
            Spacer(Modifier.height(20.dp))

            ModeControl(penOnly) { onSettingsEvent(SettingsEvent.Mode(it)); onDismiss() }

            OrientationControls(settings.orientation) { onSettingsEvent(SettingsEvent.Orientation(it)) }

            if (!penOnly) StreamControls(bitrateMbps, fpsChoice, { bitrateMbps = it }, { fpsChoice = it })

            SettingsSwitch("Battery saver", "Reduce background work while keeping your brightness and frame-rate settings.", settings.batterySaver) { onSettingsEvent(SettingsEvent.BatterySaver(it)) }
            Spacer(Modifier.height(16.dp))
            SettingsSwitch("Show stats overlay", "FPS and bandwidth in the corner", settings.showStats) { onSettingsEvent(SettingsEvent.ShowStats(it)) }
            Spacer(Modifier.height(16.dp))
            SettingsSwitch("Check for newer releases", "One request to GitHub when the app opens. Nothing installs itself.", settings.checkUpdates) { onSettingsEvent(SettingsEvent.CheckUpdates(it)) }
            Spacer(Modifier.height(24.dp))

            if (!penOnly) {
                Button(
                    onClick = {
                        onSettingsEvent(SettingsEvent.Stream((bitrateMbps * 1000).roundToInt(), fpsChoice))
                        onDismiss()
                    },
                    modifier = Modifier.fillMaxWidth(),
                    colors = ButtonDefaults.buttonColors(containerColor = Accent)
                ) {
                    Text("Apply", fontSize = 16.sp, modifier = Modifier.padding(vertical = 4.dp))
                }
                Spacer(Modifier.height(8.dp))
                Text(
                    "Applying restarts the stream for a moment.",
                    fontSize = 11.sp,
                    color = Color(0xFF6A6A7E),
                    textAlign = TextAlign.Center,
                    modifier = Modifier.fillMaxWidth()
                )
            }
            Spacer(Modifier.height(24.dp))
        }
    }
}

@Composable
private fun OrientationControls(orientation: Int, onOrientationChange: (Int) -> Unit) {
    // Which way round the tablet is held. Applies at once, in both
    // modes, and needs no Apply: it is the tablet's own business, the
    // host never sees it. Automatic uses the tilt sensor directly;
    // with it off, the two pinned directions appear.
    Row(
        verticalAlignment = Alignment.CenterVertically,
        modifier = Modifier.fillMaxWidth()
    ) {
        Column(modifier = Modifier.weight(1f)) {
            Text("Rotate automatically", fontSize = 14.sp, color = Color(0xFFB0B0C0))
            Text(
                "Follow the tilt sensor between the two landscape directions",
                fontSize = 11.sp,
                color = Color(0xFF6A6A7E)
            )
        }
        Spacer(Modifier.width(12.dp))
        Switch(
            checked = orientation == Prefs.ORIENTATION_AUTO,
            onCheckedChange = { auto ->
                onOrientationChange(
                    if (auto) Prefs.ORIENTATION_AUTO else Prefs.ORIENTATION_CAMERA_DOWN
                )
            },
            colors = SwitchDefaults.colors(checkedTrackColor = Accent)
        )
    }
    if (orientation != Prefs.ORIENTATION_AUTO) {
        Spacer(Modifier.height(8.dp))
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            listOf(
                Prefs.ORIENTATION_CAMERA_UP to "Camera up",
                Prefs.ORIENTATION_CAMERA_DOWN to "Camera down",
            ).forEach { (value, label) ->
                FilterChip(
                    selected = orientation == value,
                    onClick = { onOrientationChange(value) },
                    label = { Text(label) },
                    colors = FilterChipDefaults.filterChipColors(
                        selectedContainerColor = Accent,
                        selectedLabelColor = Color.White
                    )
                )
            }
        }
        Text(
            "Camera down is the usual way to hold it for drawing.",
            fontSize = 11.sp,
            color = Color(0xFF6A6A7E)
        )
    }
    Spacer(Modifier.height(20.dp))
}

@Composable
private fun StreamControls(bitrateMbps: Float, fpsChoice: Int, onBitrateChange: (Float) -> Unit, onFpsChange: (Int) -> Unit) {
    Text(
        "Bitrate: ${bitrateMbps.roundToInt()} Mbps",
        fontSize = 14.sp,
        color = Color(0xFFB0B0C0)
    )
    Slider(
        value = bitrateMbps,
        onValueChange = onBitrateChange,
        valueRange = (Prefs.MIN_BITRATE_KBPS / 1000).toFloat()..
                (Prefs.MAX_BITRATE_KBPS / 1000).toFloat(),
        steps = 10,
        colors = SliderDefaults.colors(thumbColor = Accent, activeTrackColor = Accent)
    )
    Text(
        "20 Mbps is plenty for text and UI. Going higher does not look sharper once " +
            "the USB link is saturated — it only adds delay.",
        fontSize = 11.sp,
        color = Color(0xFF6A6A7E)
    )
    Spacer(Modifier.height(20.dp))

    Text("Frame rate", fontSize = 14.sp, color = Color(0xFFB0B0C0))
    Spacer(Modifier.height(8.dp))
    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        // 120 is not offered: the generated EDID caps the virtual mode
        // at 90 Hz, so anything above would be duplicate frames.
        listOf(30, 60, 90).forEach { f ->
            FilterChip(
                selected = fpsChoice == f,
                onClick = { onFpsChange(f) },
                label = { Text("$f fps") },
                colors = FilterChipDefaults.filterChipColors(
                    selectedContainerColor = Accent,
                    selectedLabelColor = Color.White
                )
            )
        }
    }
    Spacer(Modifier.height(20.dp))
}

@Composable
private fun SettingsSwitch(title: String, description: String, checked: Boolean, onChange: (Boolean) -> Unit) {
    Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.fillMaxWidth()) {
        Column(modifier = Modifier.weight(1f)) {
            Text(title, fontSize = 14.sp, color = Color(0xFFB0B0C0))
            Text(description, fontSize = 11.sp, color = Color(0xFF6A6A7E))
        }
        Switch(checked = checked, onCheckedChange = onChange,
            modifier = Modifier.semantics { contentDescription = title },
            colors = SwitchDefaults.colors(checkedTrackColor = Accent))
    }
}

@Composable
private fun UpdateNotice(updateAvailable: String?, onOpenUpdate: () -> Unit) {
    if (updateAvailable != null) {
        Row(
            verticalAlignment = Alignment.CenterVertically,
            modifier = Modifier
                .fillMaxWidth()
                .clip(RoundedCornerShape(10.dp))
                .background(Color(0x3350507A))
                .clickable { onOpenUpdate() }
                .padding(12.dp)
        ) {
            Column(modifier = Modifier.weight(1f)) {
                Text("Update available: $updateAvailable", fontSize = 14.sp, color = Color.White)
                Text(
                    "Tap to open the release page. Update the desktop side too — they ship together.",
                    fontSize = 11.sp,
                    color = Color(0xFF9A9AB0)
                )
            }
        }
        Spacer(Modifier.height(16.dp))
    }
}

@Composable
private fun ModeControl(penOnly: Boolean, onChange: (Boolean) -> Unit) {
    // Stream controls fold away when the tablet is only used for drawing.
    Row(
        verticalAlignment = Alignment.CenterVertically,
        modifier = Modifier.fillMaxWidth()
    ) {
        Column(modifier = Modifier.weight(1f)) {
            Text("Graphics tablet", fontSize = 14.sp, color = Color(0xFFB0B0C0))
            Text(
                "Draw on the computer's own screen with the pen, " +
                    "instead of showing a second screen here",
                fontSize = 11.sp,
                color = Color(0xFF6A6A7E)
            )
        }
        Spacer(Modifier.width(12.dp))
        Switch(
            checked = penOnly,
            onCheckedChange = onChange,
            colors = SwitchDefaults.colors(checkedTrackColor = Accent)
        )
    }
    Spacer(Modifier.height(20.dp))
}
