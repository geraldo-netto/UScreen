package com.blent

import android.content.Intent
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
class AudioServiceTest {
    @Test fun t718_notificationStopCannotRetireReplacement() {
        checkService(1)
    }
    @Test fun t719_speakerNotificationStopCannotRetireReplacementOrMicrophone() {
        checkService(2)
    }
    private fun checkService(direction: Int) {
        val app = RuntimeEnvironment.getApplication()
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
        val invitations = MutableStateFlow<AudioEndpoint?>(null)
        val runs = mutableListOf<String?>()
        AudioOwner.reset()
        val binding = AudioBinding(app, {}, { true }, { _, _, _, _ -> awaitCancellation() }, invitations, scope, { runs.add(it) }, direction)
        val field = if (direction == 1) "microphone" else "speakers"
        ReflectionHelpers.setStaticField(AudioOwner::class.java, field, binding)
        binding.configure(AudioPreferences(background = true)); binding.start()
        val endpoint = AudioEndpoint("a".repeat(64), 12345, direction, 1, 40, true)
        invitations.value = endpoint
        val service = Robolectric.buildService(if (direction == 1) MicrophoneService::class.java else SpeakerService::class.java).create()
        try {
            val old = runs.last()
            service.get().onStartCommand(Intent().putExtra("audio_run", old), 0, 1)
            assertNull(service.get().onBind(Intent()))
            invitations.value = endpoint.copy(port = 12346)
            val current = runs.last()
            service.get().onStartCommand(Intent().putExtra("audio_run", current), 0, 2)
            service.get().onStartCommand(Intent().setAction("stop_audio").putExtra("audio_run", old), 0, 3)
            assertTrue("T718 stale notification stopped replacement", binding.sharing)
            for (invalid in listOf(null, Intent(), Intent().putExtra("audio_run", "unknown"))) {
                service.get().onStartCommand(invalid, 0, 4); assertTrue(binding.sharing)
            }
            service.get().onStartCommand(Intent().setAction("stop_audio").putExtra("audio_run", current), 0, 5)
            assertFalse(binding.sharing)
        } finally { AudioOwner.reset(); service.destroy(); scope.cancel() }
    }
}
