package com.blent

import android.Manifest
import android.media.*
import kotlinx.coroutines.runBlocking
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.*
import org.robolectric.annotation.Config
import org.robolectric.annotation.Implementation
import org.robolectric.annotation.Implements
import org.robolectric.shadows.*
import org.robolectric.util.ReflectionHelpers

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34], shadows = [AudioSpeakerTest.RoutedTrack::class])
class AudioSpeakerTest {
    // Robolectric omits the JNI route-selection return value. Real routing is
    // separately checked on the tablet; this fixture exercises our adapter.
    @Implements(AudioTrack::class)
    class RoutedTrack : ShadowAudioTrack() {
        @Implementation protected fun native_setOutputDevice(id: Int) = true
    }
    private fun attributes() = AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_MEDIA).build()
    @Test fun t719_focusDenialTransientDuckPermanentLossAndLateCallbacks() {
        val manager = RuntimeEnvironment.getApplication().getSystemService(AudioManager::class.java)
        val shadow = Shadows.shadowOf(manager)
        shadow.setNextFocusRequestResponse(AudioManager.AUDIOFOCUS_REQUEST_FAILED)
        try { AudioFocus(manager, attributes()); fail("denied focus played") } catch (_: IllegalStateException) {}
        assertNotNull(shadow.lastAbandonedAudioFocusRequest)
        shadow.setNextFocusRequestResponse(AudioManager.AUDIOFOCUS_REQUEST_GRANTED)
        val focus = AudioFocus(manager, attributes())
        val request = shadow.lastAudioFocusRequest.audioFocusRequest
        val listener = shadow.lastAudioFocusRequest.listener!!
        assertFalse(focus.paused())
        for (loss in listOf(AudioManager.AUDIOFOCUS_LOSS_TRANSIENT, AudioManager.AUDIOFOCUS_LOSS_TRANSIENT_CAN_DUCK)) {
            listener.onAudioFocusChange(loss); assertTrue(focus.paused())
            listener.onAudioFocusChange(AudioManager.AUDIOFOCUS_GAIN); assertFalse(focus.paused())
        }
        listener.onAudioFocusChange(AudioManager.AUDIOFOCUS_LOSS)
        listener.onAudioFocusChange(AudioManager.AUDIOFOCUS_GAIN)
        try { focus.paused(); fail("permanent focus loss resumed") } catch (_: IllegalStateException) {}
        focus.close(); focus.close(); assertSame(request, shadow.lastAbandonedAudioFocusRequest)
        listener.onAudioFocusChange(AudioManager.AUDIOFOCUS_GAIN)
        try { focus.paused(); fail("closed focus resumed") } catch (_: IllegalStateException) {}
    }
    @Test fun t719_nativeStereoPlaybackNeedsNoRecordPermissionAndReleases() = runBlocking {
        val app = RuntimeEnvironment.getApplication()
        Shadows.shadowOf(app).denyPermissions(Manifest.permission.RECORD_AUDIO)
        val endpoint = AudioEndpoint("a".repeat(64), 12345, 2, 1, 40, false)
        for (processing in 1..2) AudioSpeaker.open(app, endpoint.copy(processing = processing), AudioPreferences(builtIn = false)).use { player ->
            player.start(); assertFalse(player.paused)
            assertTrue(player.description().contains("native buffer"))
            assertEquals(SpeakerWrite.Written, player.write(PlaybackChunk(ShortArray(960), android.os.SystemClock.elapsedRealtime(), false)))
        }
        try { AudioSpeaker.open(app, endpoint.copy(direction = 1), AudioPreferences()); fail("microphone sent to output") } catch (_: IllegalArgumentException) {}
        try { AudioSpeaker.open(app, endpoint, AudioPreferences(gain = 101)); fail("invalid volume") } catch (_: IllegalArgumentException) {}
    }
    @Test fun t719_nativeRouteAndBufferBounds() {
        val app = RuntimeEnvironment.getApplication()
        val manager = app.getSystemService(AudioManager::class.java)
        val device = AudioDeviceInfoBuilder.newBuilder().setType(AudioDeviceInfo.TYPE_BUILTIN_SPEAKER).build()
        val port = ReflectionHelpers.getField<Any>(device, "mPort")
        ReflectionHelpers.setField(port, "mName", "fixture speaker")
        ReflectionHelpers.setField(port, "mRole", 2); ReflectionHelpers.setField(port, "mType", 2)
        Shadows.shadowOf(manager).setOutputDevices(listOf(device))
        AndroidSpeakerTrack(app, attributes(), AudioPreferences()).use { native ->
            assertNull(native.route()); native.play(); native.flush()
            ShadowAudioTrack.setRoutedDevice(device)
            assertEquals(device.id, native.route()); assertTrue(native.description().contains("native buffer"))
            assertEquals(4, native.write(byteArrayOf(1, 2, 3, 4), 0, 4).bytes)
            try { native.write(ByteArray(4), -1, 4); fail("invalid native write") } catch (_: IllegalStateException) {}
            native.close(); native.close()
        }
        ShadowAudioTrack.setRoutedDevice(null)
        Shadows.shadowOf(manager).setOutputDevices(emptyList())
        try { AndroidSpeakerTrack(app, attributes(), AudioPreferences()); fail("missing built-in speaker") } catch (_: IllegalStateException) {}
        ShadowAudioTrack.setMinBufferSize(AudioTrack.ERROR_BAD_VALUE)
        try { AndroidSpeakerTrack(app, attributes(), AudioPreferences()); fail("invalid native buffer") } catch (_: IllegalStateException) {}
    }
}
