package com.blent

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import kotlinx.coroutines.flow.MutableStateFlow

internal data class AudioEndpoint(val token: String, val port: Int, val direction: Int,
    val processing: Int, val bufferMs: Int, val background: Boolean) {
    fun valid(): Boolean = token.matches(Regex("[0-9a-f]{64}")) && port in 1..65535 &&
        direction in 1..2 && processing in 1..2 && bufferMs in 20..200 && bufferMs % 10 == 0
    companion object {
        fun read(intent: Intent): AudioEndpoint? = AudioEndpoint(intent.getStringExtra("token") ?: "",
            intent.getIntExtra("port", 0), intent.getIntExtra("direction", 0),
            intent.getIntExtra("processing", 0), intent.getIntExtra("buffer_ms", 0),
            intent.getBooleanExtra("background", false)).takeIf { it.valid() }
    }
}
internal object AudioInvitations { val microphone = MutableStateFlow<AudioEndpoint?>(null) }
class AudioReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val endpoint = AudioEndpoint.read(intent) ?: return
        if (endpoint.direction != 1) return
        AudioInvitations.microphone.value = endpoint
        resultCode = 1
    }
}
