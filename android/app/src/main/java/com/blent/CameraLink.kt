package com.blent

import java.io.DataInputStream
import java.nio.ByteBuffer
import okio.BufferedSink

/** One packet in flight; ACK means host decoder-input acceptance, never presentation. */
internal class CameraLink(private val sink: BufferedSink, private val input: DataInputStream) {
    private var sequence = 0L

    fun send(source: ByteBuffer, offset: Int, size: Int, queueAgeUs: Long): Long {
        check(sequence < Long.MAX_VALUE) { "Camera sequence exhausted" }
        val started = System.nanoTime()
        CameraWire.packet(sink, source, offset, size)
        val accepted = input.readLong()
        check(accepted == sequence + 1) { "Invalid camera feedback sequence" }
        sequence = accepted
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
