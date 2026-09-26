package com.blent

import android.os.Debug
import java.io.DataInputStream
import java.io.File
import java.net.Socket
import java.nio.ByteBuffer
import org.json.JSONArray
import org.json.JSONObject
import okio.buffer
import okio.sink

/** T607 encoded replay over an owned ADB reverse route; never opens sensors. */
internal object CameraSocketWorkloads {
    fun run(files: File, port: Int): JSONObject {
        val packets = readPackets(File(files, "camera-replay.bin"))
        val rows = JSONArray()
        val chunks = intArrayOf(8192, 12288, 16384, 32768, 65536)
        repeat(8) { trial ->
            chunks.indices.forEach { rows.put(measure(packets, chunks[(it + trial) % chunks.size], trial, port)) }
        }
        return JSONObject().put("source", ProfileBuild.SOURCE_ID).put("rows", rows)
            .put("scope", "encoded H264 payloads, maximum-throughput TCP/ADB replay, sensor off")
    }

    private fun readPackets(file: File): List<ByteBuffer> = DataInputStream(file.inputStream().buffered()).use { input ->
        val count = input.readInt()
        require(count in 1..1200)
        List(count) {
            val size = input.readInt()
            require(size in 1..CameraWire.MAX_PACKET)
            val bytes = ByteArray(size)
            input.readFully(bytes)
            ByteBuffer.allocateDirect(size).apply { put(bytes); flip() }
        }
    }

    private fun measure(packets: List<ByteBuffer>, chunk: Int, trial: Int, port: Int): JSONObject {
        Socket("127.0.0.1", port).use { socket ->
            socket.tcpNoDelay = true
            socket.soTimeout = 15000
            socket.sendBufferSize = 128 * 1024
            val sink = socket.sink().buffer()
            val count = packets.size * 10
            sink.writeInt(chunk).writeInt(trial).writeInt(count).flush()
            val input = DataInputStream(socket.getInputStream())
            check(input.readInt() == count)
            val cpu = Debug.threadCpuTimeNanos()
            val start = System.nanoTime()
            repeat(10) { packets.forEach { CameraChunkWorkloads.packet(sink, it, 0, it.capacity(), chunk) } }
            check(input.readInt() == count)
            val elapsed = System.nanoTime() - start
            val cpuElapsed = Debug.threadCpuTimeNanos() - cpu
            val bytes = packets.sumOf { it.capacity().toLong() + 4 } * 10
            return JSONObject().put("chunk", chunk).put("trial", trial).put("packets", count)
                .put("elapsed_ns", elapsed).put("cpu_ns", cpuElapsed).put("wire_bytes", bytes)
                .put("acknowledged_packets", count).put("mbps", bytes * 8000.0 / elapsed)
        }
    }
}
