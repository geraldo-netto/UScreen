package com.blent

import android.content.Context
import java.net.Socket
import java.net.InetSocketAddress
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.*
import okio.buffer
import okio.source
import okio.sink

/** Cancellation closes socket immediately; native resources retire on the IO worker. */
internal class AudioCapture(private val context: Context,
    private val open: (AudioEndpoint, AudioPreferences) -> MicrophoneDevice = { endpoint, prefs -> AudioMicrophone(context, endpoint, prefs) }) {
    suspend fun run(endpoint: AudioEndpoint, preferences: AudioPreferences, connected: (Socket?) -> Unit,
        status: (String) -> Unit) = withContext(Dispatchers.IO) {
        Socket().use { socket ->
            connected(socket)
            try {
                socket.connect(InetSocketAddress("127.0.0.1", endpoint.port), 2000)
                socket.tcpNoDelay = true
                val source = socket.source().buffer(); val sink = socket.sink().buffer()
                sink.timeout().timeout(250, TimeUnit.MILLISECONDS)
                val wire = AudioWire(endpoint, true)
                val device = try { open(endpoint, preferences) } catch (error: Exception) {
                    wire.request(sink, 0, false); throw error
                }
                device.use { microphone ->
                    wire.request(sink, microphone.capabilities, microphone.aecEnabled)
                    wire.negotiate(source)
                    microphone.start()
                    status(if (microphone.aecEnabled) "Microphone sharing · speech AEC enabled" else "Microphone sharing · AEC unavailable or raw mode")
                    val samples = ShortArray(480)
                    while (!socket.isClosed) {
                        microphone.read(samples)
                        wire.send(sink, samples, android.os.SystemClock.elapsedRealtimeNanos() / 1000, preferences.gain, microphone.clockSample())
                    }
                }
            } finally { connected(null) }
        }
    }
}
