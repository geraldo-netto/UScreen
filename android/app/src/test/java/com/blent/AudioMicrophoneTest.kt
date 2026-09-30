package com.blent

import android.Manifest
import android.media.AudioRecord
import kotlinx.coroutines.runBlocking
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.*
import org.robolectric.annotation.Config
import org.robolectric.shadows.ShadowAudioRecord

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34])
class AudioMicrophoneTest {
    private fun endpoint() = AudioEndpoint("a".repeat(64), 12345, 1, 1, 40, false)
    @Test fun t731_nativeOpenRejectsPermissionLostAfterConsent() {
        val app = RuntimeEnvironment.getApplication()
        Shadows.shadowOf(app).denyPermissions(Manifest.permission.RECORD_AUDIO)
        val error = assertThrows(SecurityException::class.java) {
            AudioMicrophone(app, endpoint(), AudioPreferences(builtIn = false)).close()
        }
        assertEquals("Microphone permission denied.", error.message)
    }
    @Test fun t718_nativeRecordPartialReadsPermissionAndRelease() = runBlocking {
        val app = RuntimeEnvironment.getApplication()
        Shadows.shadowOf(app).grantPermissions(Manifest.permission.RECORD_AUDIO)
        var reads = 0
        ShadowAudioRecord.setSource(object : ShadowAudioRecord.AudioRecordSource {
            override fun readInShortArray(array: ShortArray, offset: Int, size: Int, blocking: Boolean): Int {
                reads++
                val count = minOf(size, 120)
                array.fill(123, offset, offset + count); return count
            }
        })
        val microphone = AudioMicrophone(app, endpoint(), AudioPreferences(builtIn = false))
        try {
            assertEquals(5, microphone.capabilities); assertFalse(microphone.aecEnabled)
            microphone.start(); val samples = ShortArray(480); microphone.read(samples)
            assertEquals(4, reads); assertTrue(samples.all { it == 123.toShort() })
            try { microphone.read(ShortArray(481)); fail("invalid block") } catch (_: IllegalArgumentException) {}
            Shadows.shadowOf(app).denyPermissions(Manifest.permission.RECORD_AUDIO)
            try { microphone.read(samples); fail("revoked permission") } catch (_: IllegalStateException) {}
        } finally { microphone.close(); microphone.close(); ShadowAudioRecord.clearSource() }
        try { AudioMicrophone(app, endpoint().copy(processing = 2), AudioPreferences()); fail("raw unavailable") } catch (_: IllegalArgumentException) {}
        Shadows.shadowOf(app).grantPermissions(Manifest.permission.RECORD_AUDIO)
        try { AudioMicrophone(app, endpoint(), AudioPreferences()); fail("missing built-in route") } catch (_: IllegalStateException) {}
    }
    @Test fun t718_nativeRecordErrorCloses() = runBlocking {
        val app = RuntimeEnvironment.getApplication()
        Shadows.shadowOf(app).grantPermissions(Manifest.permission.RECORD_AUDIO)
        ShadowAudioRecord.setSource(object : ShadowAudioRecord.AudioRecordSource {
            override fun readInShortArray(array: ShortArray, offset: Int, size: Int, blocking: Boolean) = AudioRecord.ERROR_DEAD_OBJECT
        })
        try {
            AudioMicrophone(app, endpoint(), AudioPreferences(builtIn = false)).use { mic ->
                mic.start()
                try { mic.read(ShortArray(480)); fail("dead device") } catch (_: IllegalStateException) {}
            }
        } finally { ShadowAudioRecord.clearSource() }
    }
    @Test fun t718_builtinRouteAndAvailableEchoCanceler() {
        val app = RuntimeEnvironment.getApplication()
        Shadows.shadowOf(app).grantPermissions(Manifest.permission.RECORD_AUDIO)
        val manager = app.getSystemService(android.media.AudioManager::class.java)
        val device = org.robolectric.shadows.AudioDeviceInfoBuilder.newBuilder()
            .setType(android.media.AudioDeviceInfo.TYPE_BUILTIN_MIC).build()
        val port = org.robolectric.util.ReflectionHelpers.getField<Any>(device, "mPort")
        org.robolectric.util.ReflectionHelpers.setField(port, "mRole", 1)
        org.robolectric.util.ReflectionHelpers.setField(port, "mType", Int.MIN_VALUE or 4)
        Shadows.shadowOf(manager).setInputDevices(listOf(device))
        org.robolectric.shadows.ShadowAudioEffect.addEffect(android.media.audiofx.AudioEffect.Descriptor(
            android.media.audiofx.AudioEffect.EFFECT_TYPE_AEC.toString(), java.util.UUID.randomUUID().toString(), "Insert", "AEC", "test"))
        AudioMicrophone(app, endpoint(), AudioPreferences()).use { mic ->
            assertTrue(mic.aecEnabled); mic.start()
        }
        assertTrue(org.robolectric.shadows.ShadowAudioEffect.getAudioEffects().isEmpty())
    }

    @Test fun t718_routeChangesRetireInsteadOfSwitchingSilently() {
        for (first in listOf(1, 2, Int.MAX_VALUE)) {
            val route = AudioRoute(); route.observe(null); route.observe(first); route.observe(first)
            for (changed in listOf(null, -1, 0, first - 1)) {
                try { route.observe(changed); fail("T718 changed route accepted") } catch (_: IllegalStateException) {}
            }
        }
    }
    @Test @Config(sdk = [34]) fun t718_androidSilencingIsReported() = runBlocking {
        val app = RuntimeEnvironment.getApplication()
        Shadows.shadowOf(app).grantPermissions(Manifest.permission.RECORD_AUDIO)
        val manager = app.getSystemService(android.media.AudioManager::class.java)
        AudioMicrophone(app, endpoint(), AudioPreferences(builtIn = false)).use { mic ->
            val record = org.robolectric.util.ReflectionHelpers.getField<AudioRecord>(mic, "record")
            val config = Shadows.shadowOf(manager).createActiveRecordingConfiguration(record.audioSessionId, 7, app.packageName)
            org.robolectric.util.ReflectionHelpers.setField(config, "mClientSilenced", true)
            Shadows.shadowOf(manager).setActiveRecordingConfigurations(listOf(config), false)
            mic.start()
            try { mic.read(ShortArray(480)); fail("T718 silenced microphone reported healthy") }
            catch (error: IllegalStateException) { assertTrue(error.message!!.contains("silenced")) }
        }
    }

}
