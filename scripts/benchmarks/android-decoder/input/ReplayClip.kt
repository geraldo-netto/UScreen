package com.uscreen.benchmark

import java.io.DataInputStream
import java.io.File
import java.security.MessageDigest

internal class ReplayClip(file: File) {
    val width: Int
    val height: Int
    val fps: Int
    val mime: String
    val config: ByteArray
    val frames: List<ByteArray>
    val sha256: String
    init {
        require(file.length() in 1..(64L * 1024 * 1024))
        val bytes = file.readBytes()
        sha256 = MessageDigest.getInstance("SHA-256").digest(bytes).joinToString("") { "%02x".format(it) }
        DataInputStream(bytes.inputStream()).use { input ->
            val version = input.readLong()
            require(version == 0x5553444230303031L || version == 0x5553444230303032L) // USDB0001/2
            width = input.readInt(); height = input.readInt(); fps = input.readInt()
            require(width in 16..8192 && height in 16..8192 && fps in 1..90)
            val count = input.readInt()
            require(count in 1..600)
            mime = if (version == 0x5553444230303031L) "video/avc" else input.readUTF()
            require(mime in setOf("video/avc", "video/hevc", "video/x-vnd.on2.vp9", "video/av01"))
            config = packet(input, mime == "video/x-vnd.on2.vp9" || mime == "video/av01")
            frames = List(count) { packet(input) }
            require(input.read() == -1)
        }
    }
    private fun packet(input: DataInputStream, allowEmpty: Boolean = false): ByteArray {
        val size = input.readInt()
        require(size in (if (allowEmpty) 0 else 1)..(8 * 1024 * 1024))
        return ByteArray(size).also(input::readFully)
    }
}
