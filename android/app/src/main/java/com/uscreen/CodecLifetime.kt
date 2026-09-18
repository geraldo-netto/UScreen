package com.uscreen

import android.media.MediaCodec
import java.util.concurrent.ConcurrentHashMap

/** Retirement closes admission immediately. Native destruction waits for every
 * operation that borrowed the codec; a stuck driver keeps its own storage alive. */
internal class CodecLifetime(val codec: MediaCodec) {
    companion object {
        // Activity recreation replaces DecoderSession, so an unfinished native
        // retirement must close admission across receiver instances as well.
        private val retiringOwners = ConcurrentHashMap.newKeySet<CodecLifetime>()
        private var starting = false
        fun retirementPending(): Boolean = retiringOwners.isNotEmpty()
        @Synchronized fun beginStartup(): Boolean {
            if (starting || retirementPending()) return false
            starting = true
            return true
        }
        @Synchronized fun finishStartup() { starting = false }
    }
    private val gate = Object()
    private var users = 0
    @Volatile var retired = false; private set
    @Volatile private var cleanup: Thread? = null
    val finished: Boolean get() = cleanup?.isAlive == false

    fun <T> use(block: () -> T): T? {
        synchronized(gate) {
            if (retired) return null
            users++
        }
        try { return block() }
        finally { synchronized(gate) { users--; gate.notifyAll() } }
    }

    fun closeAdmission() { synchronized(gate) { retired = true; gate.notifyAll() } }

    fun retire() {
        synchronized(gate) {
            retired = true
            if (cleanup != null) return
            retiringOwners.add(this)
            cleanup = Thread({
                try { destroy() }
                finally { retiringOwners.remove(this) }
            }, "uscreen-codec-retire").apply { start() }
        }
    }

    fun awaitRetirement(milliseconds: Long) { cleanup?.join(milliseconds) }

    private fun awaitUsers() {
        synchronized(gate) {
            while (users != 0) {
                // An interrupt cannot authorize freeing borrowed native memory.
                try { gate.wait() } catch (_: InterruptedException) {}
            }
        }
    }

    private fun destroy() {
        awaitUsers()
        try { codec.stop() } catch (_: Exception) {}
        try { codec.release() } catch (_: Exception) {}
    }
}
