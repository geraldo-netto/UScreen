package com.uscreen

import android.content.Intent
import android.view.SurfaceView
import androidx.compose.animation.*
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import kotlinx.coroutines.delay

internal val Accent = Color(0xFF6C63FF)
private val AccentSoft = Color(0xFF8B85FF)
private val Ok = Color(0xFF4CAF50)
private val Warn = Color(0xFFFF9800)

@Composable
fun UScreenTheme(content: @Composable () -> Unit) {
    MaterialTheme(
        colorScheme = darkColorScheme(
            primary = Accent,
            secondary = Color(0xFF03DAC6),
            background = Color(0xFF0A0A0A),
            surface = Color(0xFF16161F),
            surfaceVariant = Color(0xFF20202C),
        )
    ) {
        content()
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun UScreenMain(
    onSurfaceReady: (SurfaceView) -> Unit,
    penOnly: Boolean = false,
    updateAvailable: String? = null,
    showThanks: Boolean = false,
    onDismissThanks: () -> Unit = {},
    onSurfaceDestroyed: () -> Unit = {},
    presentation: StreamPresentation? = null,
    settings: SettingsValues = SettingsValues(),
    displayRefreshRates: List<Float> = listOf(Prefs.DEFAULT_DISPLAY_REFRESH_RATE),
    onSettingsEvent: (SettingsEvent) -> Unit = {},
) {
    val isConnected = presentation?.connected ?: false
    var showSettings by remember { mutableStateOf(false) }
    val context = LocalContext.current

    LaunchedEffect(presentation, isConnected, settings.showStats) {
        while (isConnected) {
            delay(1000)
            presentation?.sample()
        }
    }

    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(Color.Black)
    ) {
        StreamSurface(onSurfaceReady, onSurfaceDestroyed)
        ConnectionLayers(penOnly, isConnected, controlConnected(presentation), settings.showStats, presentation?.fps ?: 0f, presentation?.mbps ?: 0f)

        // Subtle settings handle (top-right). Sits above the video surface, so
        // taps here are NOT forwarded to the Linux host.
        Box(
            modifier = Modifier
                .align(Alignment.TopEnd)
                .padding(10.dp)
                .size(38.dp)
                .alpha(if (isConnected) 0.35f else 0.9f)
                .clip(CircleShape)
                .background(Color(0xAA20202C))
                .clickable { showSettings = true },
            contentAlignment = Alignment.Center
        ) {
            Text("⚙", fontSize = 18.sp, color = Color.White)
        }

        StreamNotices(showThanks, onDismissThanks, updateAvailable)

        if (showSettings) {
            SettingsSheet(
                settings = settings,
                updateAvailable = updateAvailable,
                onOpenUpdate = {
                    openWebLink(context, UpdateCheck.RELEASES_PAGE)
                },
                penOnly = penOnly,
                onDismiss = { showSettings = false },
                displayRefreshRates = displayRefreshRates,
                onSettingsEvent = onSettingsEvent
            )
        }
    }
}

@Composable
private fun StreamSurface(onSurfaceReady: (SurfaceView) -> Unit, onSurfaceDestroyed: () -> Unit) {
    // Video surface — fills entire screen
    AndroidView(
        factory = { ctx ->
            SurfaceView(ctx).apply {
                holder.setFormat(android.graphics.PixelFormat.OPAQUE)
                holder.addCallback(
                    object : android.view.SurfaceHolder.Callback {
                        override fun surfaceCreated(holder: android.view.SurfaceHolder) {
                            onSurfaceReady(this@apply)
                        }
                        override fun surfaceChanged(
                            holder: android.view.SurfaceHolder,
                            format: Int, width: Int, height: Int
                        ) {
                            onSurfaceReady(this@apply)
                        }
                        override fun surfaceDestroyed(holder: android.view.SurfaceHolder) {
                            onSurfaceDestroyed()
                        }
                    }
                )
            }
        },
        modifier = Modifier.fillMaxSize()
    )

}

@Composable
private fun controlConnected(presentation: StreamPresentation?): Boolean =
    presentation?.controlConnected?.collectAsState()?.value ?: false

@Composable
private fun BoxScope.ConnectionLayers(penOnly: Boolean, isConnected: Boolean, controlConnected: Boolean, showStats: Boolean, fps: Float, mbps: Float) {
    // Drawing is available only while the authenticated control channel is alive.
    AnimatedVisibility(
        visible = penOnly && controlConnected,
        enter = fadeIn(),
        exit = fadeOut(),
        modifier = Modifier.fillMaxSize()
    ) {
        PenOnlyScreen()
    }

    // Connection screen
    AnimatedVisibility(
        visible = if (penOnly) !controlConnected else !isConnected,
        enter = fadeIn(),
        exit = fadeOut(),
        modifier = Modifier.fillMaxSize()
    ) {
        ConnectionScreen(penOnly)
    }

    // Stats chip (top-left, only while streaming)
    if (isConnected && showStats) {
        Surface(
            color = Color(0x99000000),
            shape = RoundedCornerShape(8.dp),
            modifier = Modifier
                .align(Alignment.TopStart)
                .padding(12.dp)
        ) {
            Text(
                text = "%.0f fps   %.1f Mbps".format(fps, mbps),
                fontSize = 12.sp,
                color = Color(0xFFB0B0C0),
                modifier = Modifier.padding(horizontal = 10.dp, vertical = 5.dp)
            )
        }
    }

}

private fun openWebLink(context: android.content.Context, url: String): Boolean {
    return try {
        context.startActivity(Intent(Intent.ACTION_VIEW, android.net.Uri.parse(url)))
        true
    } catch (_: android.content.ActivityNotFoundException) {
        android.widget.Toast.makeText(context, "No browser available to open this link.",
            android.widget.Toast.LENGTH_SHORT).show()
        false
    }
}

@Composable
private fun BoxScope.StreamNotices(showThanks: Boolean, onDismissThanks: () -> Unit, updateAvailable: String?) {
    val context = LocalContext.current
    // One-time note after the first successful picture. Dismissable, never
    // repeated: the point is one honest ask, not a nag.
    if (showThanks) {
        Box(
            modifier = Modifier
                .align(Alignment.BottomCenter)
                .padding(24.dp)
                .clip(RoundedCornerShape(14.dp))
                .background(Color(0xE620202C))
                .padding(16.dp)
        ) {
            Column {
                Text("UScreen is working.", fontSize = 15.sp, color = Color.White, fontWeight = FontWeight.Bold)
                Text(
                    "If it replaced a second monitor for you, a star on GitHub or a compatibility " +
                        "report helps other Linux users find it. This note appears only once.",
                    fontSize = 12.sp, color = Color(0xFFB0B0C0)
                )
                Row(modifier = Modifier.padding(top = 10.dp)) {
                    Text("Open GitHub", fontSize = 13.sp, color = Accent,
                        modifier = Modifier.clickable {
                            if (openWebLink(context, "https://github.com/geraldo-netto/UScreen")) {
                                onDismissThanks()
                            }
                        }.padding(end = 20.dp))
                    Text("Dismiss", fontSize = 13.sp, color = Color(0xFF9A9AB0),
                        modifier = Modifier.clickable { onDismissThanks() })
                }
            }
        }
    }

    // "Update available" pill under the settings handle. Small, and gone
    // the moment there is nothing to say.
    if (updateAvailable != null) {
        Box(
            modifier = Modifier
                .align(Alignment.TopEnd)
                .padding(top = 56.dp, end = 10.dp)
                .clip(RoundedCornerShape(14.dp))
                .background(Color(0xCC20202C))
                .clickable {
                    openWebLink(context, UpdateCheck.RELEASES_PAGE)
                }
                .padding(horizontal = 10.dp, vertical = 6.dp)
        ) {
            Text("Update $updateAvailable available", fontSize = 12.sp, color = Color.White)
        }
    }

}

@Composable
private fun PenOnlyScreen() {
    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(
                Brush.verticalGradient(
                    listOf(Color(0xFF0D0D14), Color(0xFF141B2A), Color(0xFF0D0D14))
                )
            ),
        contentAlignment = Alignment.Center
    ) {
        Column(horizontalAlignment = Alignment.CenterHorizontally) {
            Text("Graphics tablet", fontSize = 34.sp, fontWeight = FontWeight.Bold,
                color = Color.White)
            Spacer(Modifier.height(10.dp))
            Text(
                "Draw here — it goes to the screen on your computer.",
                fontSize = 15.sp, color = AccentSoft, textAlign = TextAlign.Center
            )
            Spacer(Modifier.height(28.dp))
            Text(
                "Nothing is streamed to this screen in this mode, so there is no\n" +
                    "display latency at all. Pressure, tilt and the eraser all work.",
                fontSize = 13.sp, lineHeight = 22.sp, color = Color(0xFF9A9AAE),
                textAlign = TextAlign.Center
            )
        }
    }
}

@Composable
private fun ConnectionScreen(penOnly: Boolean) {
    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(
                Brush.verticalGradient(
                    listOf(Color(0xFF0D0D14), Color(0xFF14142A), Color(0xFF0D0D14))
                )
            ),
        contentAlignment = Alignment.Center
    ) {
        Column(horizontalAlignment = Alignment.CenterHorizontally) {
            Text(
                text = "UScreen",
                fontSize = 42.sp,
                fontWeight = FontWeight.Bold,
                color = Color.White
            )
            Text(
                text = "USB second display",
                fontSize = 15.sp,
                color = AccentSoft
            )
            Spacer(modifier = Modifier.height(36.dp))
            CircularProgressIndicator(
                color = Accent,
                strokeWidth = 3.dp,
                modifier = Modifier.size(40.dp)
            )
            Spacer(modifier = Modifier.height(36.dp))
            Card(
                shape = RoundedCornerShape(16.dp),
                colors = CardDefaults.cardColors(containerColor = Color(0x8C1A1A2A))
            ) {
                Column(
                    modifier = Modifier.padding(horizontal = 28.dp, vertical = 20.dp),
                    horizontalAlignment = Alignment.CenterHorizontally
                ) {
                    Text(
                        text = if (penOnly) "Reconnecting to the host…" else "Waiting for the host…",
                        fontSize = 16.sp,
                        color = Warn,
                        fontWeight = FontWeight.Medium
                    )
                    Spacer(modifier = Modifier.height(10.dp))
                    Text(
                        text = "1. Connect the USB cable\n" +
                            "2. Allow USB debugging if asked\n" +
                            "3. Make sure uscreen is running on your PC",
                        fontSize = 13.sp,
                        lineHeight = 22.sp,
                        color = Color(0xFF9A9AAE),
                        textAlign = TextAlign.Start
                    )
                }
            }
        }
    }
}

