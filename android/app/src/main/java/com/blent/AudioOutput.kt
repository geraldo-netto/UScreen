package com.blent

import kotlinx.coroutines.delay
import kotlinx.coroutines.ensureActive
import kotlin.coroutines.coroutineContext

internal data class OutputWrite(val bytes: Int, val dead: Boolean = false)
internal interface OutputTrack : AutoCloseable {
    fun play()
    fun flush()
    fun write(bytes: ByteArray, offset: Int, count: Int): OutputWrite
    fun route(): Int?
    fun description(): String
}
internal interface OutputFocus : AutoCloseable { fun paused(): Boolean }
internal enum class SpeakerWrite { Written, Reset, Paused }
internal interface SpeakerDevice : AutoCloseable {
    val paused: Boolean
    fun start()
    fun description(): String
    suspend fun write(chunk: PlaybackChunk): SpeakerWrite
}

/** Portable playback policy. Native operations and the monotonic clock are adapters. */
internal class AudioOutput(private val open: () -> OutputTrack, private val focus: OutputFocus,
    private val clock: () -> Long) : SpeakerDevice {
    private var track: OutputTrack? = null
    private var closed = false
    private var nativePaused = true
    private var route = AudioRoute()
    private val bytes = ByteArray(1920)
    init { try { track = open() } catch (error: Exception) { focus.close(); throw error } }
    override val paused get() = focus.paused()
    override fun start() { check(!closed); synchronizeFocus() }
    override fun description() = checkNotNull(track).description()
    private fun synchronizeFocus(): Boolean {
        val pause = paused
        if (pause != nativePaused) {
            if (pause) checkNotNull(track).flush() else checkNotNull(track).play()
            nativePaused = pause
        }
        return pause
    }
    override suspend fun write(chunk: PlaybackChunk): SpeakerWrite {
        check(!closed)
        require(chunk.samples.size == 960 && chunk.at >= 0)
        if (synchronizeFocus()) return SpeakerWrite.Paused
        val now = clock()
        check(now >= chunk.at) { "Audio clock moved backwards." }
        if (now - chunk.at > 200) { flush(); return SpeakerWrite.Reset }
        if (chunk.discontinuity) flush()
        chunk.samples.forEachIndexed { index, sample ->
            bytes[index * 2] = sample.toByte(); bytes[index * 2 + 1] = (sample.toInt() shr 8).toByte()
        }
        return transfer()
    }
    private suspend fun transfer(): SpeakerWrite {
        val began = clock()
        var offset = 0
        while (offset < bytes.size) {
            coroutineContext.ensureActive()
            if (synchronizeFocus()) return SpeakerWrite.Paused
            checkWriteDeadline(began)
            val native = checkNotNull(track)
            route.observe(native.route())
            val result = native.write(bytes, offset, bytes.size - offset)
            if (result.dead) { require(result.bytes == 0); return recreate() }
            require(result.bytes in 0..(bytes.size - offset) && result.bytes % 4 == 0) { "Invalid native speaker write." }
            offset += result.bytes
            if (result.bytes == 0) delay(2)
        }
        return SpeakerWrite.Written
    }
    private fun checkWriteDeadline(began: Long) {
        val now = clock()
        check(now >= began) { "Audio clock moved backwards." }
        check(now - began < 250) { "Speaker playback stalled." }
    }
    private fun flush() {
        checkNotNull(track).flush()
        nativePaused = true
        synchronizeFocus()
    }
    private fun recreate(): SpeakerWrite {
        retireTrack()
        track = open(); route = AudioRoute(); nativePaused = true
        synchronizeFocus()
        return SpeakerWrite.Reset
    }
    private fun retireTrack() {
        val owned = track; track = null
        owned?.close()
    }
    override fun close() {
        if (closed) return
        closed = true
        try { retireTrack() } finally { focus.close() }
    }
}
