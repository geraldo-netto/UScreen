package com.blent

import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.delay
import kotlinx.coroutines.ensureActive

/** At most two retries inside the existing consent/foreground generation. */
internal object CameraRecovery {
    suspend fun run(owner: CameraResources, attempt: suspend (CameraResources) -> Nothing): Nothing {
        for (retry in 0..2) {
            currentCoroutineContext().ensureActive()
            val resources = CameraResources()
            owner.own { resources.close() }
            try {
                attempt(resources)
            } catch (error: CameraTransportException) {
                currentCoroutineContext().ensureActive()
                if (retry == 2) throw error
                android.util.Log.i("BlentCamera", "transportRetry=${retry + 1}")
            } finally { resources.close() }
            delay(250L * (retry + 1))
        }
        error("Camera retry budget exhausted")
    }
}
