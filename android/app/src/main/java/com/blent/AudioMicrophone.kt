package com.blent

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.media.*
import android.media.audiofx.AcousticEchoCanceler
import kotlinx.coroutines.delay
import kotlinx.coroutines.ensureActive
import kotlin.coroutines.coroutineContext

internal interface MicrophoneDevice : AutoCloseable {
    val capabilities: Int
    val aecEnabled: Boolean
    fun start()
    suspend fun read(output: ShortArray)
}

/** All construction, polling and retirement run on Dispatchers.IO. */
internal class AudioMicrophone(private val context: Context, private val endpoint: AudioEndpoint,
    preferences: AudioPreferences) : MicrophoneDevice {
    private val manager = context.getSystemService(AudioManager::class.java)
    private var record: AudioRecord? = null
    private var aec: AcousticEchoCanceler? = null
    private val route = AudioRoute()
    override val capabilities: Int
    override val aecEnabled get() = aec?.enabled == true
    init {
        val raw = manager.getProperty(AudioManager.PROPERTY_SUPPORT_AUDIO_SOURCE_UNPROCESSED) == "true"
        capabilities = 1 or (if (raw) 2 else 0) or 4
        require(endpoint.processing != 2 || raw) { "Raw microphone is unsupported on this tablet." }
        try { open(preferences.builtIn) } catch (error: Exception) { close(); throw error }
    }
    private fun open(builtIn: Boolean) {
        val minimum = AudioRecord.getMinBufferSize(48000, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT)
        require(minimum > 0) { "48 kHz microphone unavailable." }
        val source = if (endpoint.processing == 1) MediaRecorder.AudioSource.VOICE_COMMUNICATION else MediaRecorder.AudioSource.UNPROCESSED
        val format = AudioFormat.Builder().setSampleRate(48000).setEncoding(AudioFormat.ENCODING_PCM_16BIT)
            .setChannelMask(AudioFormat.CHANNEL_IN_MONO).build()
        record = AudioRecord.Builder().setAudioSource(source).setAudioFormat(format)
            .setBufferSizeInBytes(maxOf(minimum, 3840)).build()
        check(record!!.state == AudioRecord.STATE_INITIALIZED) { "Microphone initialization failed." }
        if (builtIn) preferBuiltIn()
        if (endpoint.processing == 1) enableAec()
    }
    override fun start() {
        record!!.startRecording()
        check(record!!.recordingState == AudioRecord.RECORDSTATE_RECORDING) { "Microphone did not start." }
    }
    private fun preferBuiltIn() {
        val input = manager.getDevices(AudioManager.GET_DEVICES_INPUTS).firstOrNull { it.type == AudioDeviceInfo.TYPE_BUILTIN_MIC }
        check(input != null && record!!.setPreferredDevice(input)) { "Built-in microphone route unavailable." }
    }
    private fun enableAec() {
        if (!AcousticEchoCanceler.isAvailable()) return
        aec = AcousticEchoCanceler.create(record!!.audioSessionId)
        aec?.enabled = true
    }
    override suspend fun read(output: ShortArray) {
        require(output.size == 480)
        val deadline = android.os.SystemClock.elapsedRealtime() + 250
        var offset = 0
        while (offset < output.size) {
            coroutineContext.ensureActive()
            check(context.checkSelfPermission(Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED) { "Microphone permission revoked." }
            val count = record!!.read(output, offset, output.size - offset, AudioRecord.READ_NON_BLOCKING)
            check(count in 0..(output.size - offset)) { "Microphone disconnected ($count)." }
            check(android.os.SystemClock.elapsedRealtime() < deadline) { "Microphone stalled." }
            healthy()
            offset += count
            if (count == 0) delay(2)
        }
    }
    private fun healthy() {
        val native = checkNotNull(record)
        route.observe(native.routedDevice?.id)
        if (android.os.Build.VERSION.SDK_INT >= 29) {
            val config = manager.activeRecordingConfigurations.firstOrNull { it.clientAudioSessionId == native.audioSessionId }
            check(config?.isClientSilenced != true) { "Android silenced microphone capture." }
        }
    }
    override fun close() {
        val effect = aec; aec = null
        val native = record; record = null
        try { effect?.release() } finally {
            native?.let { try { it.stop() } finally { it.release() } }
        }
    }
}
