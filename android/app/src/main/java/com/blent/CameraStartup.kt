// Copyright (c) 2026 Geraldo Netto
package com.blent

import java.util.concurrent.atomic.AtomicBoolean
import kotlinx.coroutines.CancellableContinuation
import kotlinx.coroutines.CancellationException
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException

/** Transfer a Camera2 callback resource before releasing its outstanding startup lease. */
internal class CameraStartup<T>(
    private val resources: CameraResources,
    private val continuation: CancellableContinuation<T>,
    private val close: (T) -> Unit,
) {
    private val release = resources.startup()
    private val delivered = AtomicBoolean()

    fun opened(value: T) = deliver(value, null)

    fun failed(value: T, message: String) {
        val error = IllegalStateException(message)
        deliver(value, error)
        resources.cancel(error)
    }

    private fun deliver(value: T, error: Exception?) {
        if (!delivered.compareAndSet(false, true)) return
        try {
            resources.own { close(value) }
            if (error == null) continuation.resume(value)
            else continuation.resumeWithException(error)
        } catch (cancelled: CancellationException) {
            continuation.cancel(cancelled)
        } finally { release() }
    }

    fun abort(error: Exception) {
        if (!delivered.compareAndSet(false, true)) return
        try { continuation.resumeWithException(error) }
        finally { release() }
    }
}
