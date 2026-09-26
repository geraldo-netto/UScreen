package com.blent

import java.util.ArrayDeque
import java.util.concurrent.TimeUnit

/** Two owned access units, including the one being submitted. A waiter owns
 * no extra detached frame; retirement wakes all producers without native I/O. */
internal class DecoderMailbox(private val clock: () -> Long = System::nanoTime) {
    data class Pending(val input: DecoderInput, val deadline: Long)
    private val gate = Object()
    private val pending = ArrayDeque<Pending>()
    private var reserved = 0
    @Volatile var closed = false; private set

    fun offer(input: DecoderInput): Boolean {
        require(input.size in 1..VideoReceiver.MAX_FRAME_SIZE)
        val deadline = clock() + TimeUnit.MILLISECONDS.toNanos(200)
        if (!reserve(deadline)) return false
        var transferred = false
        try {
            val owned = Pending(input.detached(), deadline)
            synchronized(gate) {
                if (closed) return false
                pending.addLast(owned)
                transferred = true
                return true
            }
        } finally {
            if (!transferred) synchronized(gate) { reserved--; gate.notifyAll() }
        }
    }

    private fun reserve(deadline: Long): Boolean {
        synchronized(gate) {
            while (!closed && reserved >= 2) {
                val remaining = deadline - clock()
                if (remaining <= 0) return false
                TimeUnit.NANOSECONDS.timedWait(gate, remaining)
            }
            if (closed) return false
            reserved++
            return true
        }
    }

    fun first(): Pending? = synchronized(gate) { pending.peekFirst() }
    fun complete(expected: Pending) {
        synchronized(gate) {
            if (pending.peekFirst() !== expected) return
            pending.removeFirst()
            reserved--
            gate.notifyAll()
        }
    }
    fun close() {
        synchronized(gate) {
            closed = true
            reserved -= pending.size
            pending.clear()
            gate.notifyAll()
        }
    }
}
