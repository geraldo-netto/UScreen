package com.blent

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

/** Shell-only token recovery never starts an Activity or a foreground service. */
class TokenReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        TokenDelivery.store(context, intent)
    }
}

internal object TokenDelivery {
    fun store(context: Context, intent: Intent): Boolean {
        val token = intent.getStringExtra("token") ?: return false
        if (!token.matches(Regex("[0-9a-f]{64}"))) return false
        Prefs(context).hostToken = token
        return true
    }
}
