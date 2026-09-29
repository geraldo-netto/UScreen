package com.blent

import android.content.Context

internal data class AudioPreferences(val processing: Int = 0, val background: Boolean = false,
    val gain: Int = 100, val builtIn: Boolean = true) {
    fun valid(direction: Int = 1): Boolean = direction in 1..2 && processing in 0..2 && gain in 0..(if (direction == 1) 200 else 100)
    fun effective(endpoint: AudioEndpoint): AudioEndpoint {
        require(valid(endpoint.direction))
        require(!endpoint.background || background) { "Allow background ${if (endpoint.direction == 1) "microphone" else "speaker"} sharing on the tablet first." }
        return endpoint.copy(processing = if (processing == 0) endpoint.processing else processing)
    }
    fun save(context: Context, direction: Int = 1) {
        require(valid(direction))
        context.getSharedPreferences(namespace(direction), Context.MODE_PRIVATE).edit()
            .putInt("processing", processing).putBoolean("background", background)
            .putInt("gain", gain).putBoolean("built_in", builtIn).apply()
    }
    companion object {
        private fun namespace(direction: Int): String {
            require(direction in 1..2)
            return if (direction == 1) "audio" else "audio_speakers"
        }
        fun load(context: Context, direction: Int = 1): AudioPreferences {
            val prefs = context.getSharedPreferences(namespace(direction), Context.MODE_PRIVATE)
            return AudioPreferences(prefs.getInt("processing", 0).coerceIn(0, 2), prefs.getBoolean("background", false),
                prefs.getInt("gain", 100).coerceIn(0, if (direction == 1) 200 else 100), prefs.getBoolean("built_in", true))
        }
    }
}
