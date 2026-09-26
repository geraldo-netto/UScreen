package com.blent

import android.content.Context
import android.content.Intent

/** Host-reported ADB route; loopback and charging state cannot identify it. */
internal enum class StreamTransport(val wire: String) {
    UNKNOWN("unknown"), USB("usb"), NETWORK("network");
    companion object {
        fun from(value: String?) = entries.firstOrNull { it.wire == value } ?: UNKNOWN
    }
}

internal data class ControlConnection(val authenticated: Boolean = false, val transport: StreamTransport = StreamTransport.UNKNOWN)

internal data class StreamingPower(
    val batterySaver: Boolean = false,
    val active: Boolean = false,
    val transport: StreamTransport = StreamTransport.UNKNOWN,
) {
    val needsWifi get() = !batterySaver || (active && transport != StreamTransport.USB)
    fun intent(context: Context) = Intent(context, StreamingService::class.java)
        .putExtra("battery_saver", batterySaver).putExtra("stream_connected", active)
        .putExtra("stream_transport", transport.wire)
    companion object {
        fun read(intent: Intent) = StreamingPower(intent.getBooleanExtra("battery_saver", false),
            intent.getBooleanExtra("stream_connected", false), StreamTransport.from(intent.getStringExtra("stream_transport")))
    }
}
