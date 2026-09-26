package com.blent

import android.content.Context
import android.graphics.ImageFormat
import android.graphics.SurfaceTexture
import android.hardware.camera2.*
import android.media.MediaCodec
import android.media.MediaFormat
import android.os.Handler
import android.os.Looper
import android.util.Range
import android.util.Size
import android.view.Surface
import java.net.ServerSocket
import java.nio.ByteBuffer
import java.util.concurrent.*
import kotlinx.coroutines.*
import kotlinx.coroutines.CancellationException
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.*
import org.robolectric.annotation.*
import org.robolectric.shadows.*

@Implements(MediaCodec::class)
class CameraEncoderShadow : ShadowMediaCodec() {
    companion object { var step = 0; var releases = 0; var afterPacket: () -> Unit = {}; var frames: List<Triple<Long, Long, Int>> = emptyList(); var nowUs = 0L; var syncRequests = 0; val bitrates = mutableListOf<Int>() }
    @Implementation fun createInputSurface() = Surface(SurfaceTexture(0))
    @Implementation fun dequeueOutputBuffer(info: MediaCodec.BufferInfo, timeout: Long): Int {
        if (frames.isNotEmpty() && step in 1..frames.size) {
            val (pts, now, flags) = frames[step++ - 1]
            nowUs = now
            info.set(1, 3, pts, flags)
            return 0
        }
        return when (step++) {
            0 -> MediaCodec.INFO_OUTPUT_FORMAT_CHANGED
            1 -> { info.set(1, 3, 1000, MediaCodec.BUFFER_FLAG_KEY_FRAME); 0 }
            else -> { info.set(0, 0, 2000, MediaCodec.BUFFER_FLAG_END_OF_STREAM); 0 }
        }
    }
    @Implementation fun setParameters(parameters: android.os.Bundle) {
        if (parameters.containsKey(MediaCodec.PARAMETER_KEY_REQUEST_SYNC_FRAME)) syncRequests++
        if (parameters.containsKey(MediaCodec.PARAMETER_KEY_VIDEO_BITRATE)) bitrates.add(parameters.getInt(MediaCodec.PARAMETER_KEY_VIDEO_BITRATE))
    }
    @Implementation fun getOutputBuffer(index: Int): ByteBuffer = ByteBuffer.wrap(byteArrayOf(9, 1, 2, 3, 9))
    @Implementation override fun getOutputFormat(): MediaFormat = MediaFormat().apply {
        setByteBuffer("csd-0", ByteBuffer.wrap(byteArrayOf(0, 0, 1, 0x67)))
    }
    @Implementation override fun releaseOutputBuffer(index: Int, render: Boolean) { releases++; afterPacket() }
}

@Implements(CameraManager::class)
class CameraManagerCaptureShadow {
    companion object { var failure = 0; var beforeOpen: () -> Unit = {}; var created: CameraDevice? = null }
    private val cameras = mutableMapOf<String, CameraCharacteristics>()
    fun addCamera(id: String, metadata: CameraCharacteristics) { cameras[id] = metadata }
    @Implementation fun getCameraIdList(): Array<String> = cameras.keys.toTypedArray()
    @Implementation fun getCameraCharacteristics(id: String): CameraCharacteristics = cameras.getValue(id)
    @Implementation fun openCamera(id: String, callback: CameraDevice.StateCallback, handler: Handler) {
        val camera = org.robolectric.shadow.api.Shadow.newInstanceOf(
            Class.forName("android.hardware.camera2.impl.CameraDeviceImpl")) as CameraDevice
        created = camera
        handler.post {
            beforeOpen()
            when (failure) {
                1 -> callback.onDisconnected(camera)
                2 -> callback.onError(camera, CameraDevice.StateCallback.ERROR_CAMERA_IN_USE)
                else -> callback.onOpened(camera)
            }
        }
    }
}

@Implements(className = "android.hardware.camera2.impl.CameraCaptureSessionImpl", isInAndroidSdk = false)
class CameraSessionCaptureShadow {
    var closes = 0
    @Implementation fun setRepeatingRequest(request: CaptureRequest, callback: CameraCaptureSession.CaptureCallback?, handler: Handler): Int = 1
    @Implementation fun close() { closes++ }
}

@Implements(className = "android.hardware.camera2.impl.CameraDeviceImpl", isInAndroidSdk = false)
class CameraDeviceCaptureShadow {
    var closes = 0
    @Implementation(maxSdk = 27) fun __constructor__(id: String, callback: CameraDevice.StateCallback, handler: Handler, metadata: CameraCharacteristics, target: Int) {}
    @Implementation(minSdk = 28) fun __constructor__(id: String, callback: CameraDevice.StateCallback, executor: Executor, metadata: CameraCharacteristics, physical: Map<String, CameraCharacteristics>, target: Int, context: Context) {}
    @Implementation fun getId(): String = "test"
    companion object { var failSession = false; var beforeSession: () -> Unit = {}; var created: CameraCaptureSession? = null }
    @Implementation fun createCaptureSession(surfaces: List<Surface>, callback: CameraCaptureSession.StateCallback, handler: Handler) {
        val session = org.robolectric.shadow.api.Shadow.newInstanceOf(
            Class.forName("android.hardware.camera2.impl.CameraCaptureSessionImpl")) as CameraCaptureSession
        created = session
        handler.post { beforeSession(); if (failSession) callback.onConfigureFailed(session) else callback.onConfigured(session) }
    }
    @Implementation fun createCaptureRequest(template: Int): CaptureRequest.Builder {
        val type = Class.forName("android.hardware.camera2.impl.CameraMetadataNative")
        val metadata = org.robolectric.util.ReflectionHelpers.callConstructor(type)
        val arguments = mutableListOf(
            org.robolectric.util.ReflectionHelpers.ClassParameter.from(type, metadata),
            org.robolectric.util.ReflectionHelpers.ClassParameter.from(Boolean::class.javaPrimitiveType, false),
            org.robolectric.util.ReflectionHelpers.ClassParameter.from(Int::class.javaPrimitiveType, -1))
        if (android.os.Build.VERSION.SDK_INT >= 28) {
            arguments.add(org.robolectric.util.ReflectionHelpers.ClassParameter.from(String::class.java, "test"))
            arguments.add(org.robolectric.util.ReflectionHelpers.ClassParameter.from(Set::class.java, null))
        }
        return org.robolectric.util.ReflectionHelpers.callConstructor(CaptureRequest.Builder::class.java, *arguments.toTypedArray())
    }
    @Implementation fun close() { closes++ }
}

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34], shadows = [CameraEncoderShadow::class, CameraManagerCaptureShadow::class, CameraDeviceCaptureShadow::class, CameraSessionCaptureShadow::class])
@LooperMode(LooperMode.Mode.PAUSED)
class CameraCaptureTest {
    private lateinit var manager: CameraManager
    private lateinit var capture: CameraCapture
    private val deviceCloses get() = CameraManagerCaptureShadow.created?.let {
        org.robolectric.shadow.api.Shadow.extract<CameraDeviceCaptureShadow>(it).closes
    } ?: 0
    private val sessionCloses get() = CameraDeviceCaptureShadow.created?.let {
        org.robolectric.shadow.api.Shadow.extract<CameraSessionCaptureShadow>(it).closes
    } ?: 0

    @Before fun setup() {
        val context = RuntimeEnvironment.getApplication()
        Shadows.shadowOf(context).grantPermissions(android.Manifest.permission.CAMERA)
        manager = context.getSystemService(Context.CAMERA_SERVICE) as CameraManager
        capture = CameraCapture(context)
        CameraEncoderShadow.step = 0; CameraEncoderShadow.releases = 0
        CameraEncoderShadow.afterPacket = {}
        CameraEncoderShadow.frames = emptyList(); CameraEncoderShadow.nowUs = 0; CameraEncoderShadow.syncRequests = 0; CameraEncoderShadow.bitrates.clear()
        CameraManagerCaptureShadow.failure = 0; CameraDeviceCaptureShadow.failSession = false
        CameraManagerCaptureShadow.beforeOpen = {}; CameraDeviceCaptureShadow.beforeSession = {}
        CameraManagerCaptureShadow.created = null; CameraDeviceCaptureShadow.created = null
    }

    private fun addCamera(id: String, facing: Int, size: Size = Size(1280, 720), fps: Int = 30) {
        val metadata = ShadowCameraCharacteristics.newCameraCharacteristics()
        val shadow = Shadows.shadowOf(metadata)
        shadow.set(CameraCharacteristics.LENS_FACING, facing)
        shadow.set(CameraCharacteristics.SENSOR_ORIENTATION, 90)
        shadow.set(CameraCharacteristics.CONTROL_AE_AVAILABLE_TARGET_FPS_RANGES, arrayOf(Range(fps, fps)))
        shadow.set(CameraCharacteristics.SCALER_STREAM_CONFIGURATION_MAP,
            StreamConfigurationMapBuilder.newBuilder().addOutputSize(ImageFormat.PRIVATE, size).build())
        org.robolectric.shadow.api.Shadow.extract<CameraManagerCaptureShadow>(manager).addCamera(id, metadata)
    }

    private fun exercise(lens: CameraLens = CameraLens.REAR, cancelAt: String? = null, failFeedback: Boolean = false): Pair<Throwable?, List<ByteArray>> {
        val executor = Executors.newFixedThreadPool(2)
        val packets = mutableListOf<ByteArray>()
        ServerSocket(0).use { server ->
            server.soTimeout = 2000
            val reader = executor.submit {
                repeat(if (failFeedback) 2 else 1) { connection ->
                    try {
                        server.accept().use { peer ->
                            CameraEncoderShadow.step = 0
                            peer.soTimeout = 2000
                            val input = java.io.DataInputStream(peer.getInputStream())
                            input.readFully(ByteArray(74)); peer.getOutputStream().write("OK".toByteArray())
                            var sequence = 0L
                            while (true) {
                                packets.add(ByteArray(input.readInt()).also { input.readFully(it) })
                                if (failFeedback && connection == 0) break
                                java.io.DataOutputStream(peer.getOutputStream()).writeLong(++sequence)
                            }
                        }
                    } catch (_: java.io.IOException) {}
                }
            }
            val resources = CameraResources()
            val captureJob = Job()
            if (cancelAt == "open") CameraManagerCaptureShadow.beforeOpen = { captureJob.cancel() }
            if (cancelAt == "session") CameraDeviceCaptureShadow.beforeSession = { captureJob.cancel() }
            if (cancelAt == "packet") CameraEncoderShadow.afterPacket = { captureJob.cancel() }
            val future = executor.submit<Throwable?> {
                runBlocking {
                    try {
                        withContext(captureJob) {
                            capture.run(CameraEndpoint("a".repeat(64), server.localPort, 1280, 720, 30, 3000), lens, 1, resources)
                        }
                    } catch (error: Exception) { error } finally { resources.close() }
                }
            }
            idleUntil(future)
            val failure = future.get(5, TimeUnit.SECONDS)
            reader.get(5, TimeUnit.SECONDS)
            executor.shutdownNow()
            return failure to packets
        }
    }

    private fun idleUntil(future: Future<*>) {
        val deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(5)
        while (!future.isDone && System.nanoTime() < deadline) {
            Shadows.shadowOf(Looper.getMainLooper()).idle()
            Thread.sleep(5)
        }
    }

    @Test fun t618_transportRetryStartsNewConfigurationAndFeedbackSequence() {
        addCamera("rear-id", CameraCharacteristics.LENS_FACING_BACK)
        val (failure, packets) = exercise(failFeedback = true)
        assertEquals("Camera encoder stopped", failure?.message)
        assertEquals(3, packets.size)
        assertArrayEquals(packets[0], packets[1])
        assertEquals(1, sessionCloses)
        assertEquals(1, deviceCloses)
        assertTrue(ShadowLog.getLogsForTag("BlentCamera").any { it.msg.contains("retryBitrateKbps=2250") })
    }

    @Test fun t618_sustainedQueuePressureLowersCodecTarget() {
        capture = CameraCapture(RuntimeEnvironment.getApplication()) { CameraEncoderShadow.nowUs }
        addCamera("rear-id", CameraCharacteristics.LENS_FACING_BACK)
        CameraEncoderShadow.frames = listOf(
            Triple(0L, 0L, MediaCodec.BUFFER_FLAG_KEY_FRAME),
            Triple(33_000L, 1_100_000L, 0),
            Triple(66_000L, 1_200_000L, 0),
            Triple(100_000L, 1_300_000L, 0),
            Triple(1_400_000L, 1_410_000L, MediaCodec.BUFFER_FLAG_KEY_FRAME))
        assertEquals("Camera encoder stopped", exercise().first?.message)
        assertEquals("T618 sustained pressure left bitrate unchanged", listOf(2_250_000), CameraEncoderShadow.bitrates)
    }

    @Test fun t616_staleGapDiscardsDependentFramesUntilFreshKeyframe() {
        capture = CameraCapture(RuntimeEnvironment.getApplication()) { CameraEncoderShadow.nowUs }
        addCamera("rear-id", CameraCharacteristics.LENS_FACING_BACK)
        CameraEncoderShadow.frames = listOf(
            Triple(0L, 0L, MediaCodec.BUFFER_FLAG_KEY_FRAME),
            Triple(33_000L, 250_000L, 0),
            Triple(66_000L, 260_000L, 0),
            Triple(100_000L, 270_000L, MediaCodec.BUFFER_FLAG_KEY_FRAME),
            Triple(300_000L, 310_000L, MediaCodec.BUFFER_FLAG_KEY_FRAME),
            Triple(333_000L, 340_000L, 0))
        val (failure, packets) = exercise()
        assertEquals("Camera encoder stopped", failure?.message)
        assertEquals("T616 stale/dependent packets reached the wire", 4, packets.size)
        assertEquals(1, CameraEncoderShadow.syncRequests)
        assertEquals(7, CameraEncoderShadow.releases)
    }

    @Test fun t539_nativeAdaptersSendConfigurationAndFramedCameraBytes() {
        addCamera("rear-id", CameraCharacteristics.LENS_FACING_BACK)
        val (failure, packets) = exercise()
        assertEquals("Camera encoder stopped", failure?.message)
        assertEquals(2, packets.size)
        assertArrayEquals(byteArrayOf(0, 0, 1, 0x67), packets[0])
        assertArrayEquals(byteArrayOf(1, 2, 3), packets[1])
        assertEquals(2, CameraEncoderShadow.releases)
    }

    @Test fun t539_frontCameraUsesFacingRatherThanNumericId() {
        addCamera("arbitrary-front", CameraCharacteristics.LENS_FACING_FRONT)
        assertEquals("Camera encoder stopped", exercise(CameraLens.FRONT).first?.message)
    }

    @Test fun t539_unavailableCameraAndUnsupportedSizeFailBeforeOpening() {
        assertEquals("Rear camera unavailable", exercise().first?.message)
        addCamera("rear", CameraCharacteristics.LENS_FACING_BACK, Size(640, 480))
        assertTrue(exercise().first!!.message!!.contains("does not advertise 1280x720"))
    }

    @Test fun t539_unsupportedFrameRateRetiresOpenedResources() {
        addCamera("rear", CameraCharacteristics.LENS_FACING_BACK, fps = 15)
        assertEquals("Camera does not advertise 30 FPS", exercise().first?.message)
    }

    @Test fun t539_cameraOpenAndSessionFailuresStopCapture() {
        addCamera("rear", CameraCharacteristics.LENS_FACING_BACK)
        CameraManagerCaptureShadow.failure = 1
        assertEquals("Camera disconnected", exercise().first?.message)
        CameraManagerCaptureShadow.failure = 2
        assertEquals("Camera error 1", exercise().first?.message)
        CameraManagerCaptureShadow.failure = 0
        CameraDeviceCaptureShadow.failSession = true
        assertEquals("Camera session configuration failed", exercise().first?.message)
    }

    @Test fun t539_cancelledOpenClosesTheLateCameraCallback() {
        addCamera("rear", CameraCharacteristics.LENS_FACING_BACK)
        assertTrue(exercise(cancelAt = "open").first is CancellationException)
        assertEquals(1, deviceCloses)
        assertEquals(0, sessionCloses)
    }

    @Test fun t539_cancelledConfigurationClosesLateSessionAndCamera() {
        addCamera("rear", CameraCharacteristics.LENS_FACING_BACK)
        assertTrue(exercise(cancelAt = "session").first is CancellationException)
        assertEquals(1, sessionCloses)
        assertEquals(1, deviceCloses)
    }

    @Test fun t539_cancelledActiveEncoderReleasesItsCameraAndSession() {
        addCamera("rear", CameraCharacteristics.LENS_FACING_BACK)
        val (failure, packets) = exercise(cancelAt = "packet")
        assertTrue(failure is CancellationException)
        assertEquals(2, packets.size)
        assertEquals(1, sessionCloses)
        assertEquals(1, deviceCloses)
    }
}
