package com.uscreen.benchmark

import java.io.DataInputStream
import java.io.File
import java.security.MessageDigest

internal class ReplayClip(file: File) {
    val width: Int
    val height: Int
    val fps: Int
    val config: ByteArray
    val frames: List<ByteArray>
    val sha256: String
    init {
        require(file.length() in 1..(64L * 1024 * 1024))
        val bytes = file.readBytes()
        sha256 = MessageDigest.getInstance("SHA-256").digest(bytes).joinToString("") { "%02x".format(it) }
        DataInputStream(bytes.inputStream()).use { input ->
            require(input.readLong() == 0x5553444230303031L) // USDB0001
            width = input.readInt(); height = input.readInt(); fps = input.readInt()
            require(width in 16..8192 && height in 16..8192 && fps in 1..90)
            val count = input.readInt()
            require(count in 1..600)
            config = packet(input)
            frames = List(count) { packet(input) }
            require(input.read() == -1)
        }
    }
    private fun packet(input: DataInputStream): ByteArray {
        val size = input.readInt()
        require(size in 1..(8 * 1024 * 1024))
        return ByteArray(size).also(input::readFully)
    }
}
