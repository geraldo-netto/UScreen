package com.blent

import java.io.IOException
import java.nio.ByteBuffer
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import okio.Buffer
import okio.Sink
import okio.Timeout
import okio.buffer
import org.junit.Assert.*
import org.junit.Test

/** T594: bounded packet staging, byte identity and retirement ownership. */
class CameraPacketBufferTest {
    private class RecordingSink : Sink {
        val bytes = Buffer()
        var maximumWrite = 0L
        override fun write(source: Buffer, byteCount: Long) {
            maximumWrite = maxOf(maximumWrite, byteCount)
            bytes.write(source, byteCount)
        }
        override fun timeout() = Timeout.NONE
        override fun flush() {}
        override fun close() {}
    }

    private fun packet(size: Int, direct: Boolean) {
        val expected = ByteArray(size + 16) { (it * 31).toByte() }
        val source = if (direct) ByteBuffer.allocateDirect(expected.size) else ByteBuffer.allocate(expected.size)
        source.put(expected).position(4).limit(expected.size - 3)
        val view = source.asReadOnlyBuffer().apply { mark() }
        val output = RecordingSink()
        output.buffer().use { CameraWire.packet(it, view, 7, size) }
        assertEquals(size, output.bytes.readInt())
        assertArrayEquals(expected.copyOfRange(7, size + 7), output.bytes.readByteArray())
        assertEquals(4, view.position())
        assertEquals(expected.size - 3, view.limit())
        view.reset() // Packet writes must preserve the caller's mark too.
        assertTrue("T594: one emitted segment bounds staging, got ${output.maximumWrite}", output.maximumWrite <= 8192)
    }

    @Test fun t594_payloadStagingStaysBoundedForHeapAndDirectBuffers() {
        for (size in listOf(1, 8191, 8192, 8193, CameraWire.MAX_PACKET)) {
            packet(size, false)
            packet(size, true)
        }
    }

    @Test fun t594_closeUnblocksSlowSinkAndFreshSessionHasNoStaleBytes() {
        val entered = CountDownLatch(1)
        val closed = CountDownLatch(1)
        val output = object : Sink {
            override fun write(source: Buffer, byteCount: Long) {
                entered.countDown()
                check(closed.await(5, TimeUnit.SECONDS))
                throw IOException("retired camera transport")
            }
            override fun timeout() = Timeout.NONE
            override fun flush() {}
            override fun close() { closed.countDown() }
        }
        val resources = CameraResources().apply { own { output.close() } }
        val executor = Executors.newSingleThreadExecutor()
        val source = ByteBuffer.allocateDirect(CameraWire.MAX_PACKET).apply { position(11); mark() }
        try {
            val result = executor.submit<Throwable?> {
                runCatching { CameraWire.packet(output.buffer(), source, 0, source.capacity()) }.exceptionOrNull()
            }
            assertTrue(entered.await(5, TimeUnit.SECONDS))
            resources.close()
            assertTrue(result.get(5, TimeUnit.SECONDS) is IOException)
            assertEquals(11, source.position())
            source.reset()
            packet(8193, true)
        } finally {
            resources.close()
            executor.shutdownNow()
        }
    }
}
