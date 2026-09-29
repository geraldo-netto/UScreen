package com.blent

import android.content.Intent
import java.nio.ByteBuffer
import java.nio.ByteOrder
import okio.Buffer
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34])
class AudioContractTest {
    private fun endpoint() = AudioEndpoint("a".repeat(64), 12345, 1, 1, 40, false)
    private fun hello(endpoint: AudioEndpoint = endpoint()): ByteArray {
        val bytes = ByteBuffer.allocate(92).order(ByteOrder.BIG_ENDIAN)
        bytes.put("BLAUD001".toByteArray()).put("b".repeat(64).toByteArray()).putLong(1)
        bytes.put(endpoint.direction.toByte()).put(endpoint.direction.toByte()).putInt(48000).putShort(480)
        bytes.put(endpoint.processing.toByte()).put(if (endpoint.background) 1.toByte() else 0.toByte()).putShort(endpoint.bufferMs.toShort())
        return bytes.array()
    }
    private fun rejected(action: () -> Unit) { try { action(); fail("T718 invalid input accepted") } catch (_: IllegalArgumentException) {} }
    @Test fun t718_requestBoundsAndPermissionGateManifest() {
        val app = RuntimeEnvironment.getApplication()
        val info = app.packageManager.getReceiverInfo(android.content.ComponentName(app, AudioReceiver::class.java), 0)
        assertEquals("android.permission.DUMP", info.permission)
        val base = Intent().putExtra("token", endpoint().token).putExtra("port", 12345).putExtra("direction", 1)
            .putExtra("processing", 1).putExtra("buffer_ms", 40)
        val receiver = AudioReceiver()
        app.sendOrderedBroadcast(Intent(base).setComponent(android.content.ComponentName(app, AudioReceiver::class.java)), null)
        org.robolectric.Shadows.shadowOf(android.os.Looper.getMainLooper()).idle()
        assertEquals(endpoint(), AudioInvitations.microphone.value)
        for (value in listOf(Int.MIN_VALUE, -1, 0, 1, 2, 3, Int.MAX_VALUE)) {
            assertEquals(value in 1..2, AudioEndpoint.read(Intent(base).putExtra("direction", value)) != null)
            assertEquals(value in 1..2, AudioEndpoint.read(Intent(base).putExtra("processing", value)) != null)
        }
        for (value in 0..220) assertEquals(value in 20..200 && value % 10 == 0, endpoint().copy(bufferMs = value).valid())
        for (value in listOf(-1, 0, 1, 65535, 65536, Int.MAX_VALUE)) assertEquals(value in 1..65535, endpoint().copy(port = value).valid())
        for (length in 0..128) assertEquals(length == 64, endpoint().copy(token = "a".repeat(length)).valid())
        receiver.onReceive(app, Intent())
        app.sendOrderedBroadcast(Intent(base).putExtra("direction", 2)
            .setComponent(android.content.ComponentName(app, AudioReceiver::class.java)), null)
        org.robolectric.Shadows.shadowOf(android.os.Looper.getMainLooper()).idle()
        assertEquals(endpoint().copy(direction = 2), AudioInvitations.speakers.value)
        assertEquals(endpoint(), AudioInvitations.microphone.value)
    }
    @Test fun t718_preferencesNeverPersistSessionAndInvalidSettingsReject() {
        val app = RuntimeEnvironment.getApplication()
        assertEquals(AudioPreferences(), AudioPreferences.load(app))
        val selected = AudioPreferences(2, true, 200, false); selected.save(app)
        assertEquals(selected, AudioPreferences.load(app))
        assertEquals(2, selected.effective(endpoint()).processing)
        assertEquals(endpoint(), AudioPreferences().effective(endpoint()))
        rejected { AudioPreferences().effective(endpoint().copy(background = true)) }
        rejected { selected.copy(gain = 201).save(app) }
        for (gain in -5..205) assertEquals(gain in 0..200, selected.copy(gain = gain).valid())
        app.getSharedPreferences("audio", 0).edit().putInt("processing", -1).putInt("gain", Int.MAX_VALUE).commit()
        assertTrue(AudioPreferences.load(app).valid())
    }
    @Test fun t718_handshakeBindsProfileAndPacketLayoutMatchesRust() {
        val endpoint = endpoint()
        val wire = AudioWire(endpoint)
        val request = Buffer(); wire.request(request, 7, true)
        assertEquals("BLAUREQ1", request.readUtf8(8)); assertEquals(endpoint.token, request.readUtf8(64))
        assertArrayEquals(byteArrayOf(7, 1, 1, 1), request.readByteArray())
        wire.negotiate(Buffer().writeUtf8(endpoint.token).write(hello()))
        val packet = Buffer(); val samples = ShortArray(480) { if (it % 2 == 0) Short.MIN_VALUE else Short.MAX_VALUE }
        wire.send(packet, samples, 123, 200)
        assertEquals(1L, packet.readLong()); assertEquals(0L, packet.readLong()); assertEquals(123L, packet.readLong())
        assertEquals(960, packet.readShort().toInt()); assertEquals(1, packet.readByte().toInt()); assertEquals(0, packet.readByte().toInt())
        assertEquals(Short.MIN_VALUE, packet.readShortLe()); assertEquals(Short.MAX_VALUE, packet.readShortLe())
        rejected { wire.send(Buffer(), samples, 123, 100) }
        rejected { wire.send(Buffer(), ShortArray(479), 124, 100) }
        rejected { wire.send(Buffer(), samples, 124, -1) }
        wire.send(Buffer(), samples, 124, 0)
    }
    @Test fun t718_handshakeMutationAndTruncationCorpus() {
        val endpoint = endpoint(); val original = hello()
        for (index in listOf(0, 7, 8, 72, 79, 80, 81, 82, 85, 86, 87, 88, 89, 90, 91)) {
            val changed = original.clone(); changed[index] = 0
            if (!changed.contentEquals(original)) rejected { AudioWire(endpoint).negotiate(Buffer().writeUtf8(endpoint.token).write(changed)) }
        }
        for (size in 0..155) {
            val short = (endpoint.token.toByteArray() + original).copyOf(size)
            try { AudioWire(endpoint).negotiate(Buffer().write(short)); fail("T718 truncated hello accepted") } catch (_: java.io.EOFException) {}
        }
        rejected { AudioWire(endpoint).negotiate(Buffer().writeUtf8("c".repeat(64)).write(original)) }
        rejected { AudioWire(endpoint.copy(token = "")) }
        rejected { AudioWire(endpoint).request(Buffer(), 8, false) }
    }
}
