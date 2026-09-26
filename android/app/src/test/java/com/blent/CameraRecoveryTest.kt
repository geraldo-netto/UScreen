package com.blent

import kotlinx.coroutines.*
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34])
class CameraRecoveryTest {
    @Test fun t617_retriesRetireEveryGenerationAndStopAtBound() = runBlocking {
        val owner = CameraResources()
        var attempts = 0
        var closes = 0
        try {
            CameraRecovery.run(owner) { owned ->
                assertEquals(attempts, closes)
                attempts++
                owned.own { closes++ }
                throw CameraTransportException("test")
            }
        } catch (_: CameraTransportException) {}
        owner.close()
        assertEquals(3, attempts)
        assertEquals(3, closes)
    }

    @Test fun t617_cancellationAndNonTransportFailureNeverRestart() = runBlocking {
        for (cancel in listOf(false, true)) {
            val owner = CameraResources()
            var calls = 0
            var closes = 0
            val job = launch {
                try {
                    CameraRecovery.run(owner) { owned ->
                        calls++; owned.own { closes++ }
                        if (cancel) { currentCoroutineContext().cancel(); throw CameraTransportException("closed") }
                        error("permission or codec failure")
                    }
                } catch (_: Exception) {} finally { owner.close() }
            }
            job.join()
            assertEquals(1, calls); assertEquals(1, closes)
        }
    }

    @Test fun t617_stopDuringBackoffCannotOpenAnotherCamera() = runBlocking {
        val owner = CameraResources()
        val retired = CompletableDeferred<Unit>()
        var attempts = 0
        val job = launch {
            CameraRecovery.run(owner) { owned ->
                attempts++; owned.own { retired.complete(Unit) }
                throw CameraTransportException("test")
            }
        }
        retired.await(); owner.close(); job.cancelAndJoin()
        assertEquals(1, attempts)
    }
}
