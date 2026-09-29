package com.blent

import android.content.Context

internal data class AudioPreferences(val processing: Int = 0, val background: Boolean = false,
    val gain: Int = 100, val builtIn: Boolean = true) {
    fun valid(): Boolean = processing in 0..2 && gain in 0..200
    fun effective(endpoint: AudioEndpoint): AudioEndpoint {
        require(valid())
        require(!endpoint.background || background) { "Allow background microphone sharing on the tablet first." }
        return endpoint.copy(processing = if (processing == 0) endpoint.processing else processing)
    }
    fun save(context: Context) {
        require(valid())
        context.getSharedPreferences("audio", Context.MODE_PRIVATE).edit()
            .putInt("processing", processing).putBoolean("background", background)
            .putInt("gain", gain).putBoolean("built_in", builtIn).apply()
    }
    companion object {
        fun load(context: Context): AudioPreferences {
            val prefs = context.getSharedPreferences("audio", Context.MODE_PRIVATE)
            return AudioPreferences(prefs.getInt("processing", 0).coerceIn(0, 2), prefs.getBoolean("background", false),
                prefs.getInt("gain", 100).coerceIn(0, 200), prefs.getBoolean("built_in", true))
        }
    }
}
