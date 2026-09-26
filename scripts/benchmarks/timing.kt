package com.blent

import java.lang.management.ManagementFactory
import java.util.concurrent.CyclicBarrier
import java.util.concurrent.ConcurrentLinkedQueue

private object KeepTiming { val references = ConcurrentLinkedQueue<FrameTiming>() }
private class ReplayClock(var now: Long = 1_000_000L)
private class TimingReplay(private val mode: String) {
    private val clock = ReplayClock()
    private val timing = FrameTiming { clock.now }
    private var sequence = Int.MAX_VALUE - 100
    private val step = if (mode == "sparse") 64 else 1
    private val lag = when (mode) { "delay8", "sparse" -> 8; "delay32" -> 32; "missing" -> 128; else -> 0 }
    init {
        KeepTiming.references.add(timing) // Escape: do not benchmark elided locks.
        repeat(64) { timing.noteArrival(sequence); sequence += step; clock.now += 3_000 }
    }
    fun run(count: Int): Long {
        var checksum = 0L
        repeat(count) {
            timing.noteArrival(sequence)
            clock.now += 1_000
            val target = sequence - lag * step
            timing.noteReleased(target)
            clock.now += 1_000
            checksum += timing.decodeMicrosFor(target).toLong() + 1
            clock.now += 1_000
            sequence += step
        }
        return checksum
    }
}
fun main(args: Array<String>) {
    val sessions = args[0].toInt()
    val count = args[1].toInt()
    val mode = args[2]
    val gate = CyclicBarrier(sessions)
    val rows = arrayOfNulls<String>(sessions)
    val workers = (0 until sessions).map { lane ->
        Thread {
            val replay = TimingReplay(mode)
            replay.run(100_000)
            val bean = ManagementFactory.getThreadMXBean()
            gate.await()
            val cpu = bean.currentThreadCpuTime
            val start = System.nanoTime()
            val checksum = replay.run(count)
            val wall = System.nanoTime() - start
            rows[lane] = "$lane\t$wall\t${bean.currentThreadCpuTime - cpu}\t$checksum"
        }.apply { start() }
    }
    workers.forEach { it.join() }
    rows.forEach { println(it) }
}
