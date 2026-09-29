package com.blent

import android.Manifest
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.MutableStateFlow
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.*
import org.robolectric.annotation.Config
import org.robolectric.util.ReflectionHelpers

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34])
class AudioSpeakerOwnerTest {
    @Test fun t719_preferencesAndOwnersKeepDirectionsIndependent() {
        val app = RuntimeEnvironment.getApplication()
        AudioOwner.reset()
        try {
            Shadows.shadowOf(app).denyPermissions(Manifest.permission.RECORD_AUDIO)
            val defaultBinding = AudioBinding(app, {})
            val defaultPermission = ReflectionHelpers.getField<() -> Boolean>(defaultBinding, "permission")
            assertFalse(defaultPermission())
            Shadows.shadowOf(app).grantPermissions(Manifest.permission.RECORD_AUDIO)
            assertTrue(defaultPermission()); defaultBinding.shutdown()
            Shadows.shadowOf(app).denyPermissions(Manifest.permission.RECORD_AUDIO)
            val microphone = AudioOwner.get(app)
            val speakers = AudioOwner.get(app, 2)
            assertSame(speakers, AudioOwner.get(app, 2)); assertNotSame(microphone, speakers)
            speakers.configure(AudioPreferences(2, true, 25, false))
            assertEquals(25, AudioPreferences.load(app, 2).gain)
            assertEquals(100, microphone.preferences.gain)
            assertFalse(microphone.preferences.background)
            val permission = ReflectionHelpers.getField<() -> Boolean>(speakers, "permission")
            assertTrue("T719 playback wrongly requires recording permission", permission())
            for (gain in -5..205) assertEquals(gain in 0..100, AudioPreferences(gain = gain).valid(2))
            for (direction in listOf(Int.MIN_VALUE, -1, 0, 3, Int.MAX_VALUE)) {
                try { AudioOwner.get(app, direction); fail("invalid direction") } catch (_: IllegalStateException) {}
                try { AudioPreferences.load(app, direction); fail("invalid preference namespace") } catch (_: IllegalArgumentException) {}
            }
            assertFalse(AudioOwner.owns("unknown", 2)); AudioOwner.stopSharing(2); AudioOwner.stopped("unknown", 2)
        } finally { AudioOwner.reset() }
    }
    @Test fun t719_speakerStopAndActivityHidingPreserveMicrophoneOwner() {
        val app = RuntimeEnvironment.getApplication()
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
        val microphoneRequests = MutableStateFlow<AudioEndpoint?>(null)
        val speakerRequests = MutableStateFlow<AudioEndpoint?>(null)
        val mic = AudioBinding(app, {}, { true }, { _, _, _, _ -> awaitCancellation() }, microphoneRequests, scope)
        val speaker = AudioBinding(app, {}, { true }, { _, _, _, _ -> awaitCancellation() }, speakerRequests, scope, direction = 2)
        AudioOwner.reset(); ReflectionHelpers.setStaticField(AudioOwner::class.java, "microphone", mic)
        ReflectionHelpers.setStaticField(AudioOwner::class.java, "speakers", speaker)
        try {
            val endpoint = AudioEndpoint("a".repeat(64), 12345, 1, 1, 40, false)
            mic.start(); speaker.start(); microphoneRequests.value = endpoint
            speakerRequests.value = endpoint; assertFalse(speaker.sharing); assertEquals("Invalid audio request.", speaker.status)
            speakerRequests.value = endpoint.copy(direction = 2); assertTrue(speaker.sharing)
            AudioOwner.stopSharing(2); assertTrue(mic.sharing); assertFalse(speaker.sharing)
            speakerRequests.value = endpoint.copy(direction = 2); speaker.stop()
            assertTrue(mic.sharing); assertFalse(speaker.sharing)
            speaker.start(); assertFalse(speaker.sharing)
        } finally { AudioOwner.reset(); scope.cancel() }
    }
}
