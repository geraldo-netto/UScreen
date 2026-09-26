package com.blent

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import kotlinx.coroutines.flow.MutableStateFlow

internal enum class CameraLens(val wire: Int, val label: String) { FRONT(0, "Front"), REAR(1, "Rear") }

internal data class CameraEndpoint(val token: String, val port: Int, val width: Int, val height: Int, val fps: Int, val bitrate: Int,
    val requestedLens: CameraLens? = null, val background: Boolean = false, val freshnessMs: Int = 150,
    val adaptiveBitrate: Boolean = true, val minBitrate: Int = 1000) {
    fun valid(): Boolean = token.matches(Regex("[0-9a-f]{64}")) && port in 1..65535 && validProfile() && validFreshness()

    private fun validFreshness() = freshnessMs in 50..2000 && minBitrate in 256..20000

    private fun validProfile(): Boolean = width in 160..1920 && height in 120..1080 &&
        width % 2 == 0 && height % 2 == 0 && fps in 5..30 && bitrate in 256..20000

    companion object {
        fun read(intent: Intent): CameraEndpoint? {
            val lens = intent.getIntExtra("lens", -1)
            if (lens !in -1..1) return null
            val candidate = CameraEndpoint(intent.getStringExtra("token") ?: "", intent.getIntExtra("port", 0),
                intent.getIntExtra("width", 0), intent.getIntExtra("height", 0),
                intent.getIntExtra("fps", 0), intent.getIntExtra("bitrate", 0),
                CameraLens.values().firstOrNull { it.wire == lens }, intent.getBooleanExtra("background", false), intent.getIntExtra("freshness_ms", 150),
                intent.getBooleanExtra("adaptive_bitrate", true), intent.getIntExtra("min_bitrate", 1000))
            return candidate.takeIf { it.valid() }
        }
    }
}

/** Memory-only host commands. Legacy invitations without a lens never start capture. */
internal object CameraInvitations { val endpoint = MutableStateFlow<CameraEndpoint?>(null) }

class CameraReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val endpoint = CameraEndpoint.read(intent) ?: return
        CameraInvitations.endpoint.value = endpoint
        resultCode = 1
    }
}
