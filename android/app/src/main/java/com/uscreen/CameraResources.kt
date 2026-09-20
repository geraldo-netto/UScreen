package com.uscreen

import kotlinx.coroutines.CancellationException

/** Closing races safely with startup: late resources close instead of publishing. */
internal class CameraResources {
    private val lock = Any()
    private var closed = false
    private val resources = mutableListOf<() -> Unit>()

    fun own(close: () -> Unit) {
        synchronized(lock) {
            if (!closed) { resources.add(close); return }
        }
        runCatching(close)
        throw CancellationException("Camera session stopped")
    }

    fun close() {
        val retired = synchronized(lock) {
            closed = true
            resources.toList().asReversed().also { resources.clear() }
        }
        retired.forEach { runCatching(it) }
    }
}
