package com.uscreen

import kotlinx.coroutines.CompletableDeferred
import okhttp3.*
import okio.ByteString
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34])
class DecoderNegotiationTest {
    private class Socket(val listener: WebSocketListener) : WebSocket {
        val sent = CopyOnWriteArrayList<String>()
        override fun request() = Request.Builder().url("ws://localhost/").build()
        override fun queueSize() = 0L
        override fun send(text: String): Boolean { sent.add(text); return true }
        override fun send(bytes: ByteString) = false
        override fun close(code: Int, reason: String?) = true
        override fun cancel() {}
        fun open() = listener.onOpen(this, Response.Builder().request(request()).protocol(Protocol.HTTP_1_1).code(101).message("test").build())
        fun greeting(width: Int = 640) = listener.onMessage(this,
            """{"status":"connected","codec":"h264","fps":60,"video_width":$width,"video_height":400}""")
    }
    private object Input : ControlInputState {
        override fun reset() {}
        override fun forgetTouches() {}
        override fun setTouchEnabled(enabled: Boolean) {}
        override fun setPenEnabled(enabled: Boolean) {}
    }
    @Test fun t276_sharedScaledGreetingUsesEncodedDimensionsForCapabilities() {
        lateinit var socket: Socket
        val requested = java.util.concurrent.LinkedBlockingQueue<Triple<Int, Int, Int>>()
        val control = ControlSession(Any(), Input, WebSocket.Factory { _, listener ->
            Socket(listener).also { socket = it }
        }) { width, height, fps ->
            requested.put(Triple(width, height, fps))
            DecoderCapabilities.describe(width, height, fps) { _, _, _, _ -> true }
        }
        try {
            control.connect()
            socket.open()
            socket.listener.onMessage(socket, javaClass.getResource("/control-scaled.json")!!.readText())
            assertEquals(Triple(640, 400, 30), requested.poll(2, TimeUnit.SECONDS))
            assertTrue(control.controlConnected.value)
            assertFalse(control.isPenOnly)
        } finally { control.disconnect() }
    }

    @Test fun t432_staleCapabilityQueryCannotPublishAfterReconnect() {
        val sockets = CopyOnWriteArrayList<Socket>()
        val entered = CountDownLatch(1)
        val release = CompletableDeferred<Unit>()
        val control = ControlSession(Any(), Input, WebSocket.Factory { _, listener ->
            Socket(listener).also { sockets.add(it) }
        }) { width, height, fps ->
            entered.countDown()
            release.await()
            DecoderCapabilities.describe(width, height, fps) { _, _, _, _ -> true }
        }
        try {
            control.token = "test-token"
            control.connect()
            sockets[0].open()
            sockets[0].greeting()
            assertTrue(entered.await(2, TimeUnit.SECONDS))
            control.disconnect()
            control.connect()
            sockets[1].open()
            release.complete(Unit)
            // Current request runs after the retired request is cancelled.
            sockets[1].greeting(1280)
            val deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(2)
            while (sockets[1].sent.none { it.contains("capabilities") } && System.nanoTime() < deadline) Thread.sleep(10)
            assertTrue(sockets[0].sent.none { it.contains("capabilities") })
            val message = JSONObject(sockets[1].sent.single { it.contains("capabilities") })
            assertEquals(1280, message.getJSONObject("capabilities").getInt("width"))
            assertEquals("auth", JSONObject(sockets[1].sent.first()).getString("type"))
        } finally { release.complete(Unit); control.disconnect() }
    }
}
