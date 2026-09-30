package com.blent

import android.content.Context
import android.os.SystemClock
import java.net.InetSocketAddress
import java.net.Socket
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.*
import okio.buffer
import okio.sink
import okio.source

/** Independent socket reader and bounded renderer. No IO on Android callbacks. */
internal class AudioPlayback(private val context: Context,
    private val open: (AudioEndpoint, AudioPreferences) -> SpeakerDevice = { endpoint, prefs -> AudioSpeaker.open(context, endpoint, prefs) }) {
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
                val player = try { open(endpoint, preferences) } catch (error: Exception) { wire.request(sink, 0, false); throw error }
                player.use {
                    wire.request(sink, 7, false); wire.negotiate(source)
                    currentCoroutineContext().ensureActive()
                    if (!socket.isClosed) {
                        player.start()
                        val queue = AudioPlaybackQueue(endpoint.bufferMs, SystemClock::elapsedRealtime)
                        stream(socket, wire, source, player, queue, status)
                    }
                }
            } finally { connected(null) }
        }
    }
    private suspend fun stream(socket: Socket, wire: AudioWire, source: okio.BufferedSource,
        player: SpeakerDevice, queue: AudioPlaybackQueue, status: (String) -> Unit) = coroutineScope {
        val reader = launch(Dispatchers.IO) {
            try {
                while (isActive && !socket.isClosed) {
                    val block = wire.receive(source)
                    if (!player.paused) queue.offer(block)
                }
            } catch (error: Exception) {
                currentCoroutineContext().ensureActive()
                // AudioResources closes this socket for explicit Stop. A remote
                // reset leaves isClosed false; Okio deadlines throw a timeout.
                if (error is java.net.SocketException && socket.isClosed) return@launch
                throw error
            } finally { socket.close() }
        }
        try { render(socket, player, queue, status) } finally { reader.cancel(); socket.close(); queue.clear() }
    }
    private suspend fun render(socket: Socket, player: SpeakerDevice, queue: AudioPlaybackQueue, status: (String) -> Unit) {
        val silence = ShortArray(960)
        var previous: Boolean? = null
        var reportAt = 0L
        while (currentCoroutineContext().isActive && !socket.isClosed) {
            val paused = player.paused
            val changed = previous != paused
            if (previous != null && changed) queue.clear()
            val now = SystemClock.elapsedRealtime()
            if (changed || now >= reportAt) {
                status(if (paused) "Speaker playback paused for another app." else "Speaker sharing · ${player.description()} · ${queue.driftDescription()}")
                reportAt = now + 1000
            }
            previous = paused
            renderFrame(socket, player, queue, silence)
            if (paused) delay(5)
        }
    }
    private suspend fun renderFrame(socket: Socket, player: SpeakerDevice, queue: AudioPlaybackQueue, silence: ShortArray) {
        if (socket.isClosed) return
        queue.nativeClock(player.clockSample())
        val chunk = queue.poll() ?: PlaybackChunk(silence, SystemClock.elapsedRealtime(), false)
        if (player.write(chunk) != SpeakerWrite.Written) queue.clear()
    }
}
