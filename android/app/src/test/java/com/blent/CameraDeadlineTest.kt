package com.blent

import java.net.ServerSocket
import java.nio.ByteBuffer
import java.util.concurrent.*
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34])
class CameraDeadlineTest {
    @Test fun t617_missingFeedbackRetiresConnectionWithinBudget() {
        val executor = Executors.newFixedThreadPool(2)
        val resources = CameraResources()
        val received = CountDownLatch(1)
        val retire = CountDownLatch(1)
        ServerSocket(0).use { server ->
            val peer = executor.submit {
                server.accept().use { socket ->
                    val input = java.io.DataInputStream(socket.getInputStream())
                    input.readFully(ByteArray(74)); socket.getOutputStream().write("OK".toByteArray())
                    input.readFully(ByteArray(input.readInt()))
                    received.countDown()
                    retire.await(3, TimeUnit.SECONDS)
                }
            }
            try {
                val endpoint = CameraEndpoint("a".repeat(64), server.localPort, 1280, 720, 30, 3000, freshnessMs = 50)
                val link = CameraWire.connect(endpoint, CameraLens.FRONT, 0, resources)
                val send = executor.submit<Throwable?> {
                    runCatching { link.send(ByteBuffer.wrap(byteArrayOf(1)), 0, 1, 0) }.exceptionOrNull()
                }
                assertTrue(received.await(2, TimeUnit.SECONDS))
                assertNotNull("T617 blocked ACK outlived freshness budget", send.get(500, TimeUnit.MILLISECONDS))
                assertTrue(runCatching { link.send(ByteBuffer.wrap(byteArrayOf(1)), 0, 1, 0) }.isFailure)
            } finally {
                resources.close(); retire.countDown(); peer.get(3, TimeUnit.SECONDS); executor.shutdownNow()
            }
        }
    }
    @Test fun t617_partialPacketCannotBeReusedAfterWriteDeadline() {
        val executor = Executors.newFixedThreadPool(2)
        val resources = CameraResources()
        val retire = CountDownLatch(1)
        ServerSocket().use { server ->
            server.receiveBufferSize = 1024
            server.bind(java.net.InetSocketAddress("127.0.0.1", 0))
            val peer = executor.submit<Int> {
                server.accept().use { socket ->
                    socket.soTimeout = 2000
                    val input = java.io.DataInputStream(socket.getInputStream())
                    input.readFully(ByteArray(74)); socket.getOutputStream().write("OK".toByteArray())
                    assertEquals(CameraWire.MAX_PACKET, input.readInt())
                    retire.await(2, TimeUnit.SECONDS)
                    var received = 0
                    val scratch = ByteArray(8192)
                    while (true) { val count = input.read(scratch); if (count < 0) break; received += count }
                    received
                }
            }
            try {
                val endpoint = CameraEndpoint("a".repeat(64), server.localPort, 1280, 720, 30, 3000, freshnessMs = 50)
                val link = CameraWire.connect(endpoint, CameraLens.FRONT, 0, resources)
                val send = executor.submit<Throwable?> {
                    runCatching { link.send(ByteBuffer.allocate(CameraWire.MAX_PACKET), 0, CameraWire.MAX_PACKET, 0) }.exceptionOrNull()
                }
                assertTrue(send.get(750, TimeUnit.MILLISECONDS) is CameraTransportException)
                assertTrue(runCatching { link.send(ByteBuffer.wrap(byteArrayOf(1)), 0, 1, 0) }.isFailure)
                retire.countDown()
                assertTrue(peer.get(3, TimeUnit.SECONDS) < CameraWire.MAX_PACKET)
            } finally { resources.close(); retire.countDown(); executor.shutdownNow() }
        }
    }

}
