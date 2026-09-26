package com.blent

import java.net.InetSocketAddress
import java.net.Socket
import java.nio.ByteBuffer
import java.util.concurrent.TimeUnit
import okio.BufferedSink
import okio.buffer
import okio.sink

internal object CameraWire {
    const val MAX_PACKET = 2 * 1024 * 1024

    fun connect(endpoint: CameraEndpoint, lens: CameraLens, rotation: Int, resources: CameraResources): BufferedSink {
        val socket = Socket()
        resources.own { socket.close() }
        try {
            socket.connect(InetSocketAddress("127.0.0.1", endpoint.port), 3000)
        } catch (error: java.io.IOException) {
            throw java.io.IOException("Cannot reach the computer camera service. Enable camera sharing in Blent on your computer.", error)
        }
        socket.tcpNoDelay = true
        socket.soTimeout = 3000
        socket.sendBufferSize = 128 * 1024
        val sink = socket.sink().apply { timeout().timeout(3, TimeUnit.SECONDS) }.buffer()
        greeting(sink, endpoint.token, lens, rotation)
        val input = socket.getInputStream()
        check(input.read() == 'O'.code && input.read() == 'K'.code) { "Desktop rejected camera connection" }
        return sink
    }

    fun greeting(sink: BufferedSink, token: String, lens: CameraLens, rotation: Int) {
        require(token.matches(Regex("[0-9a-f]{64}")))
        require(rotation in 0..3)
        sink.writeUtf8("BLCAM001").writeUtf8(token).writeByte(lens.wire).writeByte(rotation).flush()
    }

    fun packet(sink: BufferedSink, source: ByteBuffer, offset: Int, size: Int) {
        require(size in 1..MAX_PACKET) { "Invalid camera packet size" }
        require(offset >= 0 && offset <= source.limit() - size) { "Invalid camera buffer bounds" }
        val view = source.duplicate().apply { position(offset); limit(offset + size) }
        sink.writeInt(size)
        // Emit one Okio segment at a time. Copying the complete payload into
        // Buffer before emitting would still allocate up to MAX_PACKET bytes.
        val end = offset + size
        while (view.position() < end) {
            view.limit(minOf(end, view.position() + 8192))
            sink.write(view)
        }
        sink.flush()
    }
}
