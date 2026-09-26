package com.blent

import android.content.Intent
import java.net.ServerSocket
import java.nio.ByteBuffer
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.MutableStateFlow
import okio.Buffer
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34])
class CameraContractTest {
    private fun endpoint(port: Int = 12345) = CameraEndpoint("a".repeat(64), port, 1280, 720, 30, 3000)

    @Test fun t542_retiredDesktopEndpointExplainsHowToRestoreSharing() {
        val port = ServerSocket(0).use { it.localPort }
        val resources = CameraResources()
        try {
            val failure = runCatching {
                CameraWire.connect(endpoint(port), CameraLens.FRONT, 0, resources)
            }.exceptionOrNull()
            assertEquals("Cannot reach the computer camera service. Enable camera sharing in Blent on your computer.", failure?.message)
        } finally { resources.close() }
        // A failed connection must not prevent a later invitation/connection.
        t539_socketHandshakeAndOwnershipClose()
    }

    @Test fun t539_invitationOnlyAdvertisesAndRejectsInvalidProfiles() {
        CameraInvitations.endpoint.value = null
        val context = RuntimeEnvironment.getApplication()
        context.sendOrderedBroadcast(Intent(context, CameraReceiver::class.java), null)
        org.robolectric.Shadows.shadowOf(android.os.Looper.getMainLooper()).idle()
        assertNull(CameraInvitations.endpoint.value)
        val intent = Intent(context, CameraReceiver::class.java).putExtra("token", endpoint().token).putExtra("port", 12345)
            .putExtra("width", 1280).putExtra("height", 720).putExtra("fps", 30).putExtra("bitrate", 3000)
        context.sendOrderedBroadcast(intent, null)
        org.robolectric.Shadows.shadowOf(android.os.Looper.getMainLooper()).idle()
        assertEquals(endpoint(), CameraInvitations.endpoint.value)
        for (value in listOf(Int.MIN_VALUE, -1, 0, 1, 159, 160, 161, 1920, 1921, Int.MAX_VALUE)) {
            assertEquals(value in 160..1920 && value % 2 == 0, endpoint().copy(width = value).valid())
        }
        for (value in listOf(-1, 0, 119, 120, 121, 1080, 1081, Int.MAX_VALUE)) {
            assertEquals(value in 120..1080 && value % 2 == 0, endpoint().copy(height = value).valid())
        }
        for (value in -1..40) assertEquals(value in 5..30, endpoint().copy(fps = value).valid())
        for (value in listOf(0, 255, 256, 20000, 20001)) assertEquals(value in 256..20000, endpoint().copy(bitrate = value).valid())
        for (value in listOf(-1, 0, 1, 65535, 65536)) assertEquals(value in 1..65535, endpoint(value).valid())
        assertFalse(endpoint().copy(token = "g".repeat(64)).valid())
        for (value in listOf(Int.MIN_VALUE, 49, 50, 150, 2000, 2001, Int.MAX_VALUE))
            assertEquals(value in 50..2000, endpoint().copy(freshnessMs = value).valid())
        CameraInvitations.endpoint.value = null
    }

    @Test fun t539_wireAndBoundedInvalidInputPreserveSourceBuffer() {
        val buffer = Buffer()
        CameraWire.greeting(buffer, endpoint().token, CameraLens.REAR, 3)
        assertEquals("BLCAM002", buffer.readUtf8(8))
        assertEquals(endpoint().token, buffer.readUtf8(64))
        assertEquals(1, buffer.readByte().toInt())
        assertEquals(3, buffer.readByte().toInt())
        val bytes = ByteBuffer.wrap(byteArrayOf(1, 2, 3, 4))
        CameraWire.packet(buffer, bytes, 1, 2)
        assertEquals(2, buffer.readInt())
        assertArrayEquals(byteArrayOf(2, 3), buffer.readByteArray())
        assertEquals(0, bytes.position())
        for (offset in -2..6) for (size in -2..6) {
            val result = runCatching { CameraWire.packet(Buffer(), bytes, offset, size) }
            assertEquals(size in 1..4 && offset >= 0 && offset <= 4 - size, result.isSuccess)
        }
        assertTrue(runCatching { CameraWire.packet(buffer, bytes, 0, CameraWire.MAX_PACKET + 1) }.isFailure)
        assertTrue(runCatching { CameraWire.greeting(buffer, "wrong", CameraLens.FRONT, 0) }.isFailure)
        assertTrue(runCatching { CameraWire.greeting(buffer, endpoint().token, CameraLens.FRONT, 4) }.isFailure)
    }

    @Test fun t539_socketHandshakeAndOwnershipClose() {
        val executor = Executors.newSingleThreadExecutor()
        ServerSocket(0).use { server ->
            val received = executor.submit<ByteArray> {
                server.accept().use { peer ->
                    val greeting = ByteArray(74)
                    java.io.DataInputStream(peer.getInputStream()).readFully(greeting)
                    peer.getOutputStream().write("OK".toByteArray())
                    java.io.DataInputStream(peer.getInputStream()).readInt().let { length ->
                        ByteArray(length).also { java.io.DataInputStream(peer.getInputStream()).readFully(it); java.io.DataOutputStream(peer.getOutputStream()).writeLong(1) }
                    }
                }
            }
            val resources = CameraResources()
            val sink = CameraWire.connect(endpoint(server.localPort), CameraLens.FRONT, 0, resources)
            sink.send(ByteBuffer.wrap(byteArrayOf(4, 5)), 0, 2, 0)
            assertArrayEquals(byteArrayOf(4, 5), received.get(5, TimeUnit.SECONDS))
            resources.close()
        }
        executor.shutdownNow()
    }

    @Test fun t539_resourcesCloseOnceInReverseOrderAndRejectLateStartup() {
        val resources = CameraResources()
        val order = mutableListOf<Int>()
        resources.own { order.add(1) }
        resources.own { order.add(2); error("release failure") }
        resources.close(); resources.close()
        assertEquals(listOf(2, 1), order)
        assertTrue(runCatching { resources.own { order.add(3) } }.exceptionOrNull() is CancellationException)
        assertEquals(listOf(2, 1, 3), order)
    }

    @Test fun t539_captureRequiresForegroundConsentAndSwitchRetiresPrevious() = runBlocking {
        val invitations = MutableStateFlow<CameraEndpoint?>(endpoint())
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
        val events = mutableListOf<String>()
        var allowed = false
        var prompts = 0
        val binding = CameraBinding(RuntimeEnvironment.getApplication(), { 1 }, { prompts++ }, { allowed },
            { _, lens, rotation, resources ->
                assertEquals(1, rotation)
                events.add("open ${lens.label}")
                resources.own { events.add("close ${lens.label}") }
                awaitCancellation()
            }, invitations, scope)
        binding.choose(CameraLens.FRONT)
        assertEquals(0, prompts)
        binding.start(); binding.start()
        assertNull(binding.selected)
        binding.choose(CameraLens.FRONT)
        assertEquals(1, prompts)
        binding.permissionResult(false)
        assertEquals("Camera permission denied.", binding.status)
        assertTrue(events.isEmpty())
        binding.choose(CameraLens.FRONT)
        allowed = true
        binding.permissionResult(true)
        assertEquals(CameraLens.FRONT, binding.selected)
        binding.choose(CameraLens.REAR)
        assertEquals(listOf("open Front", "close Front", "open Rear"), events)
        invitations.value = endpoint(12346)
        assertNull(binding.selected)
        assertEquals("close Rear", events.last())
        binding.choose(CameraLens.REAR)
        binding.stop()
        assertNull(binding.endpoint)
        assertNull(binding.selected)
        binding.permissionResult(true)
        binding.start()
        assertNull(binding.selected)
        binding.choose(null)
        binding.stop(); scope.cancel()
    }

    @Test fun t539_errorsStopCameraAndInvitationsNeverEnableCapture() = runBlocking {
        val invitations = MutableStateFlow<CameraEndpoint?>(null)
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
        val binding = CameraBinding(RuntimeEnvironment.getApplication(), { 0 }, {}, { true },
            { _, _, _, _ -> error("Camera busy") }, invitations, scope)
        binding.start(); binding.choose(CameraLens.FRONT)
        assertNull(binding.selected)
        invitations.value = endpoint()
        assertNull(binding.selected)
        binding.choose(CameraLens.FRONT)
        assertEquals("Camera busy", binding.status)
        assertNull(binding.selected)
        invitations.value = null
        assertNull(binding.endpoint)
        binding.stop(); scope.cancel()
    }

    @Test fun t539_frontAndRearRotationUsesSensorAndDisplay() {
        for (display in 0..3) {
            assertEquals(((90 - display * 90 + 360) % 360) / 90, CameraCapture.rotation(90, display, CameraLens.REAR))
            assertEquals(((270 + display * 90) % 360) / 90, CameraCapture.rotation(270, display, CameraLens.FRONT))
        }
    }
}
