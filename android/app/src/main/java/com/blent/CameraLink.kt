package com.blent

import java.io.IOException
import java.nio.ByteBuffer
import java.util.concurrent.TimeUnit
import okio.BufferedSink
import okio.BufferedSource

internal class CameraTransportException(message: String, cause: Throwable? = null) : IOException(message, cause)

/** One packet in flight; ACK means host decoder-input acceptance, never presentation. */
internal class CameraLink(private val sink: BufferedSink, private val input: BufferedSource,
    private val budgetMs: Int = 150, private val close: () -> Unit = {}) {
    private var sequence = 0L
    private var active = true

    init { require(budgetMs in 50..2000) }

    fun send(source: ByteBuffer, offset: Int, size: Int, queueAgeUs: Long): Long {
        check(active) { "Camera connection retired" }
        check(sequence < Long.MAX_VALUE) { "Camera sequence exhausted" }
        require(queueAgeUs in 0..budgetMs * 1000L)
        try {
            return exchange(source, offset, size, queueAgeUs)
        } catch (error: IOException) {
            active = false
            runCatching { close() }
            throw CameraTransportException("Camera transport stalled or disconnected", error)
        }
    }

    private fun exchange(source: ByteBuffer, offset: Int, size: Int, queueAgeUs: Long): Long {
        val started = System.nanoTime()
        val deadline = started + TimeUnit.MILLISECONDS.toNanos(budgetMs.toLong()) - queueAgeUs * 1000
        sink.timeout().deadlineNanoTime(deadline)
        input.timeout().deadlineNanoTime(deadline)
        try {
            CameraWire.packet(sink, source, offset, size)
            if (input.readLong() != sequence + 1) throw IOException("Invalid camera feedback sequence")
            if (System.nanoTime() >= deadline) throw java.net.SocketTimeoutException("Camera feedback exceeded freshness budget")
            sequence++
            return report(started, queueAgeUs)
        } finally {
            sink.timeout().clearDeadline()
            input.timeout().clearDeadline()
        }
    }

    private fun report(started: Long, queueAgeUs: Long): Long {
        val elapsedUs = (System.nanoTime() - started) / 1000
        if (sequence % 30 == 0L) android.util.Log.i("BlentCamera",
            "accepted=$sequence queueAgeUs=$queueAgeUs feedbackUs=$elapsedUs ageBasis=relative-encoder-queue")
        return elapsedUs
    }
}

/** Camera→encoder timestamp compensation varies by device. Estimate extra queue
 * age against the first encoded frame, using sender clocks only. Initial capture
 * and encoding delay is excluded; this is deliberately not capture/output age. */
internal class CameraFrameClock {
    private var firstPts = -1L
    private var firstNow = 0L
    private var lastPts = -1L

    fun ageUs(pts: Long, now: Long): Long {
        require(pts >= 0 && now >= 0) { "Invalid camera timestamp" }
        require(pts >= lastPts) { "Camera timestamp moved backwards" }
        if (firstPts < 0) { firstPts = pts; firstNow = now }
        require(now >= firstNow) { "Camera clock moved backwards" }
        lastPts = pts
        return ((now - firstNow) - (pts - firstPts)).coerceAtLeast(0)
    }
}
