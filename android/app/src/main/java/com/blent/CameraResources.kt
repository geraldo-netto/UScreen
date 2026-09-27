package com.blent

import java.util.concurrent.atomic.AtomicBoolean
import kotlinx.coroutines.CancellationException

/** The capture worker owns native destruction; cancellation only interrupts transport. */
internal class CameraResources {
    private val lock = java.lang.Object()
    private var stopping = false
    private var failure: Exception? = null
    private var closed = false
    private var closing = false
    private var startups = 0
    private val resources = mutableListOf<() -> Unit>()
    private val interrupts = mutableListOf<() -> Unit>()

    fun own(close: () -> Unit) {
        val late = synchronized(lock) {
            if (!closed) resources.add(close)
            closed
        }
        if (late) runCatching(close)
        synchronized(lock) {
            if (stopping) throw CancellationException("Camera session stopped")
        }
    }

    /** Only nonblocking transport interruption belongs here, never codec destruction. */
    fun interrupt(close: () -> Unit) {
        synchronized(lock) {
            if (!stopping) { interrupts.add(close); return }
        }
        runCatching(close)
        throw CancellationException("Camera session stopped")
    }

    /** Keep retirement waiting for a Camera2 callback even after coroutine cancellation. */
    fun startup(): () -> Unit {
        synchronized(lock) {
            if (stopping) throw CancellationException("Camera session stopped")
            startups++
        }
        val released = AtomicBoolean()
        return {
            if (released.compareAndSet(false, true)) synchronized(lock) {
                startups--
                lock.notifyAll()
            }
        }
    }

    fun checkActive() {
        synchronized(lock) {
            failure?.let { throw it }
            if (stopping) throw CancellationException("Camera session stopped")
        }
    }

    fun cancel(error: Exception? = null) {
        val pending = synchronized(lock) {
            if (failure == null) failure = error
            stopping = true
            interrupts.toList().also { interrupts.clear() }
        }
        pending.forEach { runCatching(it) }
    }

    /** Call only after the capture worker has left its borrowed native operations. */
    fun close() {
        cancel()
        val retired = synchronized(lock) {
            while (startups != 0 || closing) lock.wait()
            closed = true
            closing = true
            resources.toList().asReversed().also { resources.clear() }
        }
        try { retired.forEach { runCatching(it) } }
        finally { synchronized(lock) { closing = false; lock.notifyAll() } }
    }
}
