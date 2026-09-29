package com.blent

import kotlinx.coroutines.*
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.*
import org.robolectric.annotation.Config
import java.net.ServerSocket
import java.net.Socket
import java.nio.ByteBuffer
import java.util.concurrent.atomic.AtomicBoolean

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34])
class AudioCaptureTest {
    @Test fun t718_permissionReadyGrantPrecedesCaptureAndDisconnectReleases() = runBlocking {
        val app = RuntimeEnvironment.getApplication()
        for (aec in listOf(false, true)) ServerSocket(0).use { server ->
            val endpoint = AudioEndpoint("a".repeat(64), server.localPort, 1, 1, 40, false)
            val started = AtomicBoolean(false); val closed = AtomicBoolean(false)
            var attached: Socket? = null; var reads = 0; var detail = ""
            val capture = AudioCapture(app) { _, _ -> object : MicrophoneDevice {
                override val capabilities = 5
                override val aecEnabled = aec
                override fun start() { started.set(true) }
                override suspend fun read(output: ShortArray) {
                    check(started.get()); if (reads++ > 0) error("fixture done")
                    output.fill(123)
                }
                override fun close() { closed.set(true) }
            } }
            val peer = async(Dispatchers.IO) {
                server.accept().use { socket ->
                    val input = socket.getInputStream(); assertEquals(76, input.readNBytes(76).size)
                    assertFalse("T718 microphone started before authenticated grant", started.get())
                    val hello = ByteBuffer.allocate(92).put("BLAUD001".toByteArray()).put("b".repeat(64).toByteArray())
                        .putLong(1).put(1).put(1).putInt(48000).putShort(480).put(1).put(0).putShort(40).array()
                    socket.getOutputStream().write(endpoint.token.toByteArray() + hello)
                    val packet = input.readNBytes(988)
                    assertEquals(988, packet.size); assertEquals(123.toByte(), packet[28])
                }
            }
            try { capture.run(endpoint, AudioPreferences(), { attached = it }, { detail = it }); fail("expected EOF") }
            catch (error: IllegalStateException) { assertEquals("fixture done", error.message) }
            peer.await(); assertNull(attached); assertTrue(closed.get()); assertTrue(detail.contains("Microphone sharing"))
        }
    }
    @Test fun t718_nativeOpenFailureReportsUnsupportedAndClosesSocket() = runBlocking {
        ServerSocket(0).use { server ->
            val endpoint = AudioEndpoint("a".repeat(64), server.localPort, 1, 1, 40, false)
            val peer = async(Dispatchers.IO) { server.accept().use { it.getInputStream().readNBytes(76) } }
            var attached: Socket? = Socket()
            val capture = AudioCapture(RuntimeEnvironment.getApplication()) { _, _ -> error("unavailable") }
            try { capture.run(endpoint, AudioPreferences(), { attached = it }, {}); fail("open failure") }
            catch (error: IllegalStateException) { assertEquals("unavailable", error.message) }
            assertEquals(0, peer.await()[72].toInt()); assertNull(attached)
        }
    }
    @Test fun t718_stopAtReadinessRetiresWithoutTakingFirstSample() = runBlocking(Dispatchers.IO) {
        ServerSocket(0).use { server ->
            val endpoint = AudioEndpoint("a".repeat(64), server.localPort, 1, 1, 40, false)
            var attached: Socket? = null; var closed = false
            val capture = AudioCapture(RuntimeEnvironment.getApplication()) { _, _ -> object : MicrophoneDevice {
                override val capabilities = 5
                override val aecEnabled = false
                override fun start() {}
                override suspend fun read(output: ShortArray) { fail("T718 Stop allowed a new native read") }
                override fun close() { closed = true }
            } }
            val peer = async(Dispatchers.IO) { server.accept().use { socket ->
                socket.getInputStream().readNBytes(76)
                val hello = ByteBuffer.allocate(92).put("BLAUD001".toByteArray()).put("b".repeat(64).toByteArray())
                    .putLong(1).put(1).put(1).putInt(48000).putShort(480).put(1).put(0).putShort(40).array()
                socket.getOutputStream().write(endpoint.token.toByteArray() + hello)
                assertEquals(-1, socket.getInputStream().read())
            } }
            capture.run(endpoint, AudioPreferences(), { attached = it }, { attached!!.close() })
            peer.await(); assertTrue(closed); assertNull(attached)
        }
    }

}
