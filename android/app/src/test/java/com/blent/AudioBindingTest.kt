package com.blent

import kotlinx.coroutines.*
import kotlinx.coroutines.flow.MutableStateFlow
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import java.net.Socket

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34])
class AudioBindingTest {
    private fun endpoint() = AudioEndpoint("a".repeat(64), 12345, 1, 1, 40, false)
    @Test fun t718_permissionSettingsAndForegroundStopAreExplicit() = runBlocking {
        val commands = MutableStateFlow<AudioEndpoint?>(null)
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
        var permission = false; var prompts = 0; var opens = 0; var closes = 0
        val binding = AudioBinding(RuntimeEnvironment.getApplication(), { prompts++ }, { permission },
            { _, _, socket, status -> opens++; socket(Socket()); status("Ready"); try { awaitCancellation() } finally { closes++ } }, commands, scope)
        binding.start(); binding.start(); assertEquals(0, opens)
        commands.value = endpoint(); assertEquals(1, prompts)
        binding.permissionResult(false); assertNull(binding.pending); assertEquals(0, opens)
        commands.value = endpoint(); permission = true; binding.permissionResult(true)
        assertEquals(1, opens); assertEquals("Ready", binding.status); assertTrue(binding.sharing)
        binding.stop(); assertFalse(binding.sharing); assertEquals(1, closes)
        binding.start(); assertEquals(1, opens)
        commands.value = endpoint(); assertEquals(2, opens)
        binding.configure(binding.preferences.copy(gain = 50)); assertFalse(binding.sharing); assertEquals(2, closes)
        assertEquals(50, binding.preferences.gain)
        binding.accept(); binding.permissionResult(true); assertEquals(2, opens)
        commands.value = endpoint().copy(port = 0); assertEquals("Invalid audio request.", binding.status)
        binding.shutdown(); scope.cancel()
    }
    @Test fun t718_backgroundRequiresConsentAndOldServiceCannotStopReplacement() = runBlocking {
        val commands = MutableStateFlow<AudioEndpoint?>(null)
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
        val runs = mutableListOf<String?>()
        val binding = AudioBinding(RuntimeEnvironment.getApplication(), {}, { true }, { _, _, _, _ -> awaitCancellation() }, commands, scope, { runs.add(it) })
        binding.start(); commands.value = endpoint().copy(background = true)
        assertFalse(binding.sharing); assertTrue(binding.status.contains("Allow background"))
        binding.configure(binding.preferences.copy(background = true))
        commands.value = endpoint().copy(background = true)
        assertTrue(binding.sharing); val old = runs.last(); assertTrue(binding.ownsBackground(old))
        binding.stop(); assertTrue(binding.sharing)
        binding.start(); binding.stopSharing(); commands.value = endpoint().copy(background = true)
        binding.backgroundStopped(old); assertTrue(binding.sharing)
        binding.backgroundStopped(runs.last()); assertFalse(binding.sharing)
        binding.shutdown(); scope.cancel()
    }
    @Test fun t718_retirementCompletesBeforeReplacementNativeOpen() = runBlocking {
        val commands = MutableStateFlow<AudioEndpoint?>(null)
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
        val retirement = CompletableDeferred<Unit>(); var opens = 0; var failedRun: String? = null
        val binding = AudioBinding(RuntimeEnvironment.getApplication(), {}, { true }, { _, _, _, _ ->
            opens++; try { awaitCancellation() } finally { withContext(NonCancellable) { retirement.await() } }
        }, commands, scope)
        binding.start(); commands.value = endpoint(); assertEquals(1, opens)
        commands.value = endpoint().copy(port = 12346); assertEquals(1, opens)
        retirement.complete(Unit); yield(); assertEquals(2, opens)
        binding.shutdown(); scope.cancel()
    }
    @Test fun t718_cancelBeforeSocketRegistrationClosesLateResource() {
        val resources = AudioResources(); resources.cancel()
        val socket = Socket(); resources.attach(socket); assertTrue(socket.isClosed)
        resources.attach(null); resources.cancel()
        val active = AudioResources(); val other = Socket(); active.attach(other); active.cancel(); assertTrue(other.isClosed)
    }
    @Test fun t718_failedBackgroundPromotionDoesNotCapture() {
        val commands = MutableStateFlow<AudioEndpoint?>(null)
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
        var opens = 0; var failedRun: String? = null
        val binding = AudioBinding(RuntimeEnvironment.getApplication(), {}, { true }, { _, _, _, _ -> opens++ }, commands, scope, { if (it != null) { failedRun = it; error("foreground denied") } })
        binding.configure(binding.preferences.copy(background = true)); binding.start()
        commands.value = endpoint().copy(background = true)
        assertEquals(0, opens); assertEquals("foreground denied", binding.status)
        assertFalse("T718 failed promotion retains no background ownership", binding.ownsBackground(failedRun))
        assertFalse(binding.sharing)
        binding.shutdown(); scope.cancel()
    }
    @Test fun t718_optionalServiceCallbackCanRetireConsentedBackgroundSession() {
        val invitations = MutableStateFlow<AudioEndpoint?>(null)
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
        val binding = AudioBinding(RuntimeEnvironment.getApplication(), {}, { true }, { _, _, _, _ -> awaitCancellation() }, invitations, scope)
        binding.configure(AudioPreferences(background = true)); binding.start()
        invitations.value = endpoint().copy(background = true); assertTrue(binding.sharing)
        binding.stopSharing(); assertFalse(binding.sharing); binding.shutdown()
    }

}
