package com.blent

import android.content.Context
import android.hardware.camera2.*
import android.media.MediaCodec
import android.media.MediaCodecInfo
import android.media.MediaFormat
import android.os.Handler
import android.os.Looper
import android.util.Range
import android.util.Size
import android.view.Surface
import kotlinx.coroutines.*
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException

/** Camera2/MediaCodec resources belong to one foreground, explicitly selected run. */
@OptIn(ExperimentalCoroutinesApi::class)
internal class CameraCapture(context: Context, private val nowUs: () -> Long = { System.nanoTime() / 1000 }) {
    private val manager = context.getSystemService(Context.CAMERA_SERVICE) as CameraManager
    private val handler = Handler(Looper.getMainLooper())

    suspend fun stream(endpoint: CameraEndpoint, lens: CameraLens, displayRotation: Int, resources: CameraResources): Nothing =
        withContext(Dispatchers.IO) {
            val id = cameraId(lens)
            val metadata = manager.getCameraCharacteristics(id)
            validateSize(metadata, endpoint)
            val sensor = metadata.get(CameraCharacteristics.SENSOR_ORIENTATION) ?: 0
            val rotation = rotation(sensor, displayRotation, lens)
            android.util.Log.i("BlentCamera", "timestampSource=${metadata.get(CameraCharacteristics.SENSOR_INFO_TIMESTAMP_SOURCE)} age=relative-encoder-queue")
            val sink = CameraWire.connect(endpoint, lens, rotation, resources)
            val codec = encoder(endpoint, resources)
            val surface = codec.createInputSurface()
            resources.own { surface.release() }
            codec.start()
            val camera = withTimeout(5000) { open(id).also { device -> resources.own { device.close() } } }
            val session = withTimeout(5000) { session(camera, surface).also { configured -> resources.own { configured.close() } } }
            repeat(camera, session, surface, frameRate(metadata, endpoint.fps))
            drain(codec, sink, endpoint.freshnessMs)
        }

    private fun cameraId(lens: CameraLens): String {
        val facing = if (lens == CameraLens.FRONT) CameraCharacteristics.LENS_FACING_FRONT else CameraCharacteristics.LENS_FACING_BACK
        return manager.cameraIdList.firstOrNull { manager.getCameraCharacteristics(it).get(CameraCharacteristics.LENS_FACING) == facing }
            ?: error("${lens.label} camera unavailable")
    }

    private fun validateSize(metadata: CameraCharacteristics, endpoint: CameraEndpoint) {
        val sizes = metadata.get(CameraCharacteristics.SCALER_STREAM_CONFIGURATION_MAP)?.getOutputSizes(MediaCodec::class.java)
        require(sizes?.contains(Size(endpoint.width, endpoint.height)) == true) { "Camera does not advertise ${endpoint.width}x${endpoint.height}; choose another desktop camera profile" }
    }

    private fun frameRate(metadata: CameraCharacteristics, fps: Int): Range<Int> {
        val ranges = metadata.get(CameraCharacteristics.CONTROL_AE_AVAILABLE_TARGET_FPS_RANGES).orEmpty()
        return ranges.filter { it.contains(fps) }.minByOrNull { it.upper - it.lower }
            ?: error("Camera does not advertise $fps FPS")
    }

    private fun encoder(endpoint: CameraEndpoint, resources: CameraResources): MediaCodec {
        val codec = MediaCodec.createEncoderByType("video/avc")
        resources.own { closeEncoder(codec) }
        val format = MediaFormat.createVideoFormat("video/avc", endpoint.width, endpoint.height).apply {
            setInteger(MediaFormat.KEY_COLOR_FORMAT, MediaCodecInfo.CodecCapabilities.COLOR_FormatSurface)
            setInteger(MediaFormat.KEY_BIT_RATE, endpoint.bitrate * 1000)
            setInteger(MediaFormat.KEY_FRAME_RATE, endpoint.fps)
            setInteger(MediaFormat.KEY_I_FRAME_INTERVAL, 1)
            setInteger(MediaFormat.KEY_PROFILE, MediaCodecInfo.CodecProfileLevel.AVCProfileBaseline)
        }
        codec.configure(format, null, null, MediaCodec.CONFIGURE_FLAG_ENCODE)
        return codec
    }

    private fun closeEncoder(codec: MediaCodec) {
        try { codec.stop() } finally { codec.release() }
    }

    @android.annotation.SuppressLint("MissingPermission")
    private suspend fun open(id: String): CameraDevice = suspendCancellableCoroutine { continuation ->
        manager.openCamera(id, object : CameraDevice.StateCallback() {
            override fun onOpened(camera: CameraDevice) { continuation.resume(camera) { camera.close() } }
            override fun onDisconnected(camera: CameraDevice) { camera.close(); if (continuation.isActive) continuation.resumeWithException(IllegalStateException("Camera disconnected")) }
            override fun onError(camera: CameraDevice, error: Int) { camera.close(); if (continuation.isActive) continuation.resumeWithException(IllegalStateException("Camera error $error")) }
        }, handler)
    }

    @Suppress("DEPRECATION")
    private suspend fun session(camera: CameraDevice, surface: Surface): CameraCaptureSession = suspendCancellableCoroutine { continuation ->
        camera.createCaptureSession(listOf(surface), object : CameraCaptureSession.StateCallback() {
            override fun onConfigured(session: CameraCaptureSession) { continuation.resume(session) { session.close() } }
            override fun onConfigureFailed(session: CameraCaptureSession) { session.close(); if (continuation.isActive) continuation.resumeWithException(IllegalStateException("Camera session configuration failed")) }
        }, handler)
    }

    private fun repeat(camera: CameraDevice, session: CameraCaptureSession, surface: Surface, fps: Range<Int>) {
        val request = camera.createCaptureRequest(CameraDevice.TEMPLATE_RECORD).apply {
            addTarget(surface)
            set(CaptureRequest.CONTROL_AE_TARGET_FPS_RANGE, fps)
        }.build()
        session.setRepeatingRequest(request, null, handler)
    }

    private suspend fun drain(codec: MediaCodec, sink: CameraLink, freshnessMs: Int): Nothing {
        val info = MediaCodec.BufferInfo()
        val clock = CameraFrameClock()
        val freshness = CameraFreshness(freshnessMs * 1000L)
        var progress = android.os.SystemClock.elapsedRealtime()
        while (true) {
            currentCoroutineContext().ensureActive()
            val index = codec.dequeueOutputBuffer(info, 10_000)
            when {
                index >= 0 -> { sendBuffer(codec, sink, info, index, clock, freshness); progress = android.os.SystemClock.elapsedRealtime() }
                index == MediaCodec.INFO_OUTPUT_FORMAT_CHANGED -> sendConfiguration(codec.outputFormat, sink)
            }
            check(android.os.SystemClock.elapsedRealtime() - progress < 5000) { "Camera encoder stopped producing frames" }
        }
    }

    private fun sendBuffer(codec: MediaCodec, sink: CameraLink, info: MediaCodec.BufferInfo, index: Int, clock: CameraFrameClock, freshness: CameraFreshness) {
        try {
            if (info.size > 0) {
                val buffer = requireNotNull(codec.getOutputBuffer(index))
                sendFrame(codec, sink, buffer, info, clock, freshness)
            }
            check(info.flags and MediaCodec.BUFFER_FLAG_END_OF_STREAM == 0) { "Camera encoder stopped" }
        } finally { codec.releaseOutputBuffer(index, false) }
    }

    private fun sendFrame(codec: MediaCodec, sink: CameraLink, buffer: java.nio.ByteBuffer,
        info: MediaCodec.BufferInfo, clock: CameraFrameClock, freshness: CameraFreshness) {
        if (info.flags and MediaCodec.BUFFER_FLAG_CODEC_CONFIG != 0) {
            sink.send(buffer, info.offset, info.size, 0)
            return
        }
        val now = nowUs()
        val age = clock.ageUs(info.presentationTimeUs, now)
        val key = info.flags and MediaCodec.BUFFER_FLAG_KEY_FRAME != 0
        when (freshness.choose(age, key, now)) {
            CameraFreshness.Decision.SEND -> sink.send(buffer, info.offset, info.size, age)
            CameraFreshness.Decision.DROP -> Unit
            CameraFreshness.Decision.REQUEST_SYNC -> {
                android.util.Log.i("BlentCamera", "freshnessGap dropped=${freshness.dropped} queueAgeUs=$age")
                codec.setParameters(android.os.Bundle().apply { putInt(MediaCodec.PARAMETER_KEY_REQUEST_SYNC_FRAME, 0) })
            }
        }
    }

    private fun sendConfiguration(format: MediaFormat, sink: CameraLink) {
        for (key in listOf("csd-0", "csd-1")) {
            val buffer = format.getByteBuffer(key) ?: continue
            sink.send(buffer, buffer.position(), buffer.remaining(), 0)
        }
    }

    companion object {
        fun rotation(sensor: Int, display: Int, lens: CameraLens): Int {
            val degrees = (display.coerceIn(0, 3) * 90)
            val adjusted = if (lens == CameraLens.FRONT) sensor + degrees else sensor - degrees
            return ((adjusted + 360) % 360) / 90
        }
    }
}
