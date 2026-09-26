package com.blent.benchmark

import java.io.File
import java.io.RandomAccessFile
import java.nio.ByteBuffer
import java.nio.channels.FileChannel

/** T419 trusted research fixture, fixed RGB888 geometry; no production protocol. */
internal class RectClip(file: File, val mapped: Boolean) : AutoCloseable {
    private val input = RandomAccessFile(file, "r")
    val bytes: ByteBuffer
    val codec: Int
    val width: Int
    val height: Int
    val rate: Int
    val frames: List<RectFrame>
    init {
        try {
            require(input.length() in 24..(256L * 1024 * 1024))
            require(input.readInt() == 0x54523431) // TR41
            codec = input.readInt(); width = input.readInt(); height = input.readInt(); rate = input.readInt()
            val count = input.readInt()
            require(codec in 1..2 && width == 1280 && height == 800)
            require(rate in setOf(5, 60) && count in 1..600)
            frames = List(count) { RectFrame.read(input, width, height, it) }
            require(input.filePointer == input.length())
            bytes = if (mapped) input.channel.map(FileChannel.MapMode.READ_ONLY, 0, input.length())
                else ByteBuffer.allocateDirect(frames.maxOf { it.length })
        } catch (error: Exception) { input.close(); throw error }
    }
    fun dataOffset(frame: RectFrame): Int {
        if (mapped) return frame.offset
        bytes.clear(); bytes.limit(frame.length)
        var position = frame.offset.toLong()
        while (bytes.hasRemaining()) {
            val read = input.channel.read(bytes, position)
            check(read > 0) { "Truncated fixture" }
            position += read
        }
        return 0
    }
    override fun close() { input.close() }
}

internal data class RectFrame(val x: Int, val y: Int, val width: Int, val height: Int,
                              val offset: Int, val length: Int, val hash: Long) {
    val rawBytes get() = width * height * 3
    companion object {
        fun read(input: RandomAccessFile, width: Int, height: Int, number: Int): RectFrame {
            val x = input.readInt(); val y = input.readInt(); val w = input.readInt(); val h = input.readInt()
            val length = input.readInt(); val hash = input.readLong()
            require(x in 0..width && y in 0..height)
            require(w in 0..(width - x) && h in 0..(height - y))
            require(length in 0..3_100_000 && length <= input.length() - input.filePointer)
            validateUpdate(x, y, w, h, length, width, height, number)
            val result = RectFrame(x, y, w, h, input.filePointer.toInt(), length, hash)
            input.seek(input.filePointer + length)
            return result
        }
        private fun validateUpdate(x: Int, y: Int, w: Int, h: Int, length: Int,
                                   width: Int, height: Int, number: Int) {
            require((length == 0) == (w == 0 && h == 0))
            require(length == 0 || (w > 0 && h > 0))
            if (number == 0) require(x == 0 && y == 0 && w == width && h == height)
        }
    }
}

internal object RectNative {
    init { System.loadLibrary("rect_decode") }
    external fun create(): Long
    external fun destroy(context: Long)
    external fun decode(context: Long, codec: Int, input: ByteBuffer, offset: Int,
                        bytes: Int, output: ByteBuffer, expected: Int): Boolean
    external fun rgbHash(buffer: ByteBuffer, pixels: Int): Long
    external fun enableTimestamps(display: Long, surface: Long): Boolean
    external fun nextFrame(display: Long, surface: Long): Long
    external fun presented(display: Long, surface: Long, id: Long): Long
}
