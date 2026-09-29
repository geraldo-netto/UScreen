package com.blent

import java.nio.ByteBuffer
import java.nio.ByteOrder
import okio.BufferedSource
import okio.BufferedSink

/** V1 matches common/src/audio/wire.rs. Credentials never enter saved preferences. */
internal class AudioWire(private val endpoint: AudioEndpoint) {
    init { require(endpoint.valid()) }
    private var generation = 0L
    private var sequence = 0L
    private var lastTime = -1L
    fun request(sink: BufferedSink, capabilities: Int, aec: Boolean) {
        require(capabilities in 0..7)
        sink.writeUtf8("BLAUREQ1").writeUtf8(endpoint.token).writeByte(capabilities)
            .writeByte(if (aec) 1 else 0).writeByte(endpoint.direction).writeByte(endpoint.processing).flush()
    }
    fun negotiate(source: BufferedSource) {
        // T724: one native-readiness budget covers the nonce and complete grant.
        source.timeout().timeout(5, java.util.concurrent.TimeUnit.SECONDS)
            .deadline(5, java.util.concurrent.TimeUnit.SECONDS)
        try { readGrant(source) } finally { source.timeout().clearDeadline() }
    }
    private fun readGrant(source: BufferedSource) {
        require(java.security.MessageDigest.isEqual(source.readByteArray(64), endpoint.token.toByteArray(Charsets.US_ASCII))) { "Audio host authentication failed" }
        val bytes = source.readByteArray(92)
        require(String(bytes, 0, 8, Charsets.US_ASCII) == "BLAUD001")
        require(String(bytes, 8, 64, Charsets.US_ASCII).matches(Regex("[0-9a-f]{64}")))
        val fields = ByteBuffer.wrap(bytes).order(ByteOrder.BIG_ENDIAN); fields.position(72)
        generation = fields.long
        require(generation != 0L)
        require(fields.get().toInt() == endpoint.direction && fields.get().toInt() == endpoint.direction)
        require(fields.int == 48000 && fields.short.toInt() == 480)
        require(fields.get().toInt() == endpoint.processing && fields.get().toInt() == if (endpoint.background) 1 else 0)
        require(fields.short.toInt() == endpoint.bufferMs)
    }
    fun send(sink: BufferedSink, pcm: ShortArray, timestampUs: Long, gain: Int) {
        require(generation != 0L && sequence < Long.MAX_VALUE)
        require(pcm.size == 480 * endpoint.direction && gain in 0..200)
        require(timestampUs > lastTime)
        sink.writeLong(generation).writeLong(sequence).writeLong(timestampUs)
            .writeShort(pcm.size * 2).writeByte(endpoint.direction).writeByte(0)
        for (sample in pcm) sink.writeShortLe((sample.toInt() * gain / 100).coerceIn(-32768, 32767))
        sink.flush(); sequence++; lastTime = timestampUs
    }
}
