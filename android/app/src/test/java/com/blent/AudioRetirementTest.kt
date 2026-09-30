package com.blent

import kotlinx.coroutines.*
import kotlinx.coroutines.flow.MutableStateFlow
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34])
class AudioRetirementTest {
    @Test fun t732_microphoneReplacementsWaitForEveryNativeClose() = checkRetirement(1, 0)
    @Test fun t732_speakerReplacementsWaitForEveryNativeClose() = checkRetirement(2, 0)
    @Test fun t732_microphoneStopRetiresWithoutOpeningQueuedRequests() = checkRetirement(1, 1)
    @Test fun t732_speakerStopRetiresWithoutOpeningQueuedRequests() = checkRetirement(2, 1)
    @Test fun t732_microphoneShutdownRetiresWithoutOpeningQueuedRequests() = checkRetirement(1, 2)
    @Test fun t732_speakerShutdownRetiresWithoutOpeningQueuedRequests() = checkRetirement(2, 2)

    private fun checkRetirement(direction: Int, ending: Int) = runBlocking {
        val commands = MutableStateFlow<AudioEndpoint?>(null)
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
        val retire = CompletableDeferred<Unit>()
        var opens = 0; var closes = 0
        val binding = AudioBinding(RuntimeEnvironment.getApplication(), {}, { true },
            { _, _, _, _ ->
                opens++
                try { awaitCancellation() }
                finally { withContext(NonCancellable) { retire.await(); closes++ } }
            }, commands, scope, direction = direction)
        val endpoint = AudioEndpoint("a".repeat(64), 12345, direction, 1, 40, false)
        try {
            binding.start(); commands.value = endpoint
            assertEquals(1, opens)
            for (replacement in 1..8) {
                commands.value = endpoint.copy(port = 12345 + replacement)
                assertEquals("T732 replacement bypassed native retirement", 1, opens)
                assertEquals(0, closes)
            }
            when (ending) {
                1 -> binding.stopSharing()
                2 -> binding.shutdown()
            }
            retire.complete(Unit); yield()
            assertEquals(1, closes)
            assertEquals(if (ending == 0) 2 else 1, opens)
            if (ending == 1) {
                commands.value = endpoint
                assertEquals("T732 fresh Start after complete retirement", 2, opens)
            }
        } finally {
            retire.complete(Unit); binding.shutdown(); scope.cancel()
        }
        assertEquals(opens, closes)
    }
}
