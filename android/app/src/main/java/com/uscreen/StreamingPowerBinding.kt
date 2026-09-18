package com.uscreen

import android.content.Context
import android.content.Intent
import android.util.Log
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.Flow

/** Main-thread Activity ownership; no retired observer can restart the service. */
internal class StreamingPowerBinding(private val context: Context) {
    private var scope: CoroutineScope? = null
    private var active = false

    fun start(initial: StreamingPower, requests: Flow<StreamingPower>) {
        if (active) return
        if (!send(initial, foreground = true)) return
        active = true
        val owner = CoroutineScope(Dispatchers.Main.immediate + SupervisorJob())
        scope = owner
        owner.launch {
            requests.collect { request ->
                if (active && !send(request, foreground = false)) stop()
            }
        }
    }

    private fun send(power: StreamingPower, foreground: Boolean): Boolean = try {
        val intent = power.intent(context)
        val started = if (foreground) context.startForegroundService(intent) else context.startService(intent)
        started != null
    } catch (error: Exception) {
        Log.w("UScreenService", "Service start rejected", error)
        false
    }

    fun stop() {
        if (!active) return
        active = false
        scope?.cancel()
        scope = null
        context.stopService(Intent(context, StreamingService::class.java))
    }
}
