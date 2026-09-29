package com.blent

import android.content.Context
import android.media.*
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import java.util.concurrent.atomic.AtomicInteger

/** OS focus notifications only update state; native playback stays on its IO owner. */
internal class AudioFocus(private val manager: AudioManager, attributes: AudioAttributes) : OutputFocus {
    private val state = AtomicInteger(0)
    private val request = AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN)
        .setAudioAttributes(attributes).setWillPauseWhenDucked(true)
        .setOnAudioFocusChangeListener(::changed, Handler(Looper.getMainLooper())).build()
    init {
        if (manager.requestAudioFocus(request) != AudioManager.AUDIOFOCUS_REQUEST_GRANTED) {
            close(); error("Speaker audio focus denied.")
        }
        state.compareAndSet(0, 1)
    }
    private fun changed(value: Int) {
        val next = when (value) {
            AudioManager.AUDIOFOCUS_GAIN -> 1
            AudioManager.AUDIOFOCUS_LOSS_TRANSIENT, AudioManager.AUDIOFOCUS_LOSS_TRANSIENT_CAN_DUCK -> 2
            else -> 3
        }
        state.updateAndGet { current -> if (current >= 3) current else next }
    }
    override fun paused(): Boolean {
        val current = state.get()
        check(current < 3) { "Speaker audio focus lost. Start again on your computer." }
        return current != 1
    }
    override fun close() {
        if (state.getAndSet(4) != 4) manager.abandonAudioFocusRequest(request)
    }
}

/** AudioTrack and route details stay behind the portable OutputTrack interface. */
internal class AndroidSpeakerTrack(context: Context, attributes: AudioAttributes,
    preferences: AudioPreferences) : OutputTrack {
    private val manager = context.getSystemService(AudioManager::class.java)
    private var track: AudioTrack? = null
    private var preferred: Int? = null
    init {
        try {
            val minimum = AudioTrack.getMinBufferSize(48000, AudioFormat.CHANNEL_OUT_STEREO, AudioFormat.ENCODING_PCM_16BIT)
            check(minimum > 0) { "48 kHz stereo playback unavailable." }
            val format = AudioFormat.Builder().setSampleRate(48000).setEncoding(AudioFormat.ENCODING_PCM_16BIT)
                .setChannelMask(AudioFormat.CHANNEL_OUT_STEREO).build()
            track = AudioTrack.Builder().setAudioAttributes(attributes).setAudioFormat(format)
                .setTransferMode(AudioTrack.MODE_STREAM).setBufferSizeInBytes(maxOf(minimum, 7680)).build()
            check(track!!.state == AudioTrack.STATE_INITIALIZED) { "Speaker initialization failed." }
            check(track!!.setVolume(preferences.gain / 100f) == AudioTrack.SUCCESS)
            if (preferences.builtIn) preferBuiltIn()
        } catch (error: Exception) { close(); throw error }
    }
    private fun preferBuiltIn() {
        val device = manager.getDevices(AudioManager.GET_DEVICES_OUTPUTS).firstOrNull { it.type == AudioDeviceInfo.TYPE_BUILTIN_SPEAKER }
        check(device != null && track!!.setPreferredDevice(device)) { "Built-in speaker route unavailable." }
        preferred = device.id
    }
    override fun play() {
        checkNotNull(track).play()
        check(track!!.playState == AudioTrack.PLAYSTATE_PLAYING) { "Speaker playback did not start." }
    }
    override fun flush() { checkNotNull(track).pause(); track!!.flush() }
    override fun write(bytes: ByteArray, offset: Int, count: Int): OutputWrite {
        val result = checkNotNull(track).write(bytes, offset, count, AudioTrack.WRITE_NON_BLOCKING)
        if (result == AudioTrack.ERROR_DEAD_OBJECT) return OutputWrite(0, true)
        check(result >= 0) { "Speaker write failed ($result)." }
        return OutputWrite(result)
    }
    override fun route(): Int? {
        val actual = checkNotNull(track).routedDevice?.id
        check(preferred == null || actual == null || preferred == actual) { "Built-in speaker route was not honored." }
        return actual
    }
    override fun description(): String {
        val native = checkNotNull(track)
        val route = native.routedDevice?.productName ?: "route pending"
        val played = native.playbackHeadPosition.toLong() and 0xffffffffL
        return "$route · native buffer ${native.bufferSizeInFrames} frames · played $played frames · underruns ${native.underrunCount}"
    }
    override fun close() {
        val native = track; track = null
        native?.let { try { it.pause(); it.flush() } finally { it.release() } }
    }
}

internal object AudioSpeaker {
    fun open(context: Context, endpoint: AudioEndpoint, preferences: AudioPreferences): SpeakerDevice {
        require(endpoint.direction == 2 && preferences.gain in 0..100)
        val speech = endpoint.processing == 1
        val attributes = AudioAttributes.Builder()
            .setUsage(if (speech) AudioAttributes.USAGE_VOICE_COMMUNICATION else AudioAttributes.USAGE_MEDIA)
            .setContentType(if (speech) AudioAttributes.CONTENT_TYPE_SPEECH else AudioAttributes.CONTENT_TYPE_MUSIC).build()
        val manager = context.getSystemService(AudioManager::class.java)
        return AudioOutput({ AndroidSpeakerTrack(context, attributes, preferences) },
            AudioFocus(manager, attributes), SystemClock::elapsedRealtime)
    }
}
