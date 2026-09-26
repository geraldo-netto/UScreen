package com.blent

import java.io.InputStream
import java.net.InetSocketAddress
import java.net.Socket

/** Owns a published socket, including a connection blocked in connect/read.
 * Retirement always names its socket so an old worker cannot close a new run. */
internal class VideoTransport(
    private val monitor: Any,
    private val current: (Long) -> Boolean,
    private val createSocket: () -> Socket,
) {
    private var socket: Socket? = null

    fun newConnection(): Socket = createSocket()

    fun connect(generation: Long, connection: Socket): InputStream? {
        synchronized(monitor) {
            if (!current(generation)) return null
            socket = connection
        }
        connection.connect(InetSocketAddress(VideoReceiver.HOST, VideoReceiver.PORT), 5000)
        connection.apply {
            tcpNoDelay = true
            soTimeout = 10000
            // Expose backpressure rather than buffer seconds of stale video.
            receiveBufferSize = 128 * 1024
        }
        val input = connection.getInputStream()
        synchronized(monitor) {
            if (!current(generation)) return null
        }
        return input
    }

    fun retire(connection: Socket?) {
        try { connection?.close() } catch (_: Exception) {}
        synchronized(monitor) {
            if (socket === connection) socket = null
        }
    }

    // Caller owns the monitor for the whole codec/generation handoff.
    fun interrupt() {
        try { socket?.close() } catch (_: Exception) {}
    }

    fun stop() {
        interrupt()
        socket = null
    }
}
