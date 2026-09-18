package com.uscreen

import android.graphics.SurfaceTexture
import android.media.MediaCodec
import android.view.Surface
import okhttp3.*
import okio.ByteString
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34], shadows = [StartupCodecShadow::class])
class StreamFormatTest {
    private class Socket(val listener: WebSocketListener) : WebSocket {
        override fun request() = Request.Builder().url("ws://localhost/").build()
        override fun queueSize() = 0L
        override fun send(text: String) = true
        override fun send(bytes: ByteString) = true
        override fun close(code: Int, reason: String?) = true
        override fun cancel() {}
        fun greet(text: String) = listener.onMessage(this, text)
    }
    private class Fixture(dispatch: ((() -> Unit) -> Unit) = { it() }) : AutoCloseable {
        val sockets = mutableListOf<Socket>()
        val capture = TouchCapture(WebSocket.Factory { _, listener -> Socket(listener).also { sockets.add(it) } })
        val receiver = VideoReceiver { error("T466: no physical video socket") }
        val session = SessionCoordinator(Prefs(RuntimeEnvironment.getApplication()), dispatch, receiver, capture)
        init { session.nativeResolution(3840, 2160, 300, 190); session.start() }
        fun greet(width: Int = 640, height: Int = 400, fps: Int = 30) {
            sockets.last().greet("""{"codec":"h264","fps":$fps,"video_width":$width,"video_height":$height,"pen_only":true}""")
        }
        override fun close() = session.stop()
    }

    @Test fun t466_scaledFormatReachesDecoderAndNativeUpdatesCannotReplaceIt() {
        StartupCodecShadow.stage = ""
        Fixture().use { f ->
            f.greet()
            f.session.nativeResolution(3840, 2160, 300, 190)
            val attempted = mutableListOf<DecoderFormat>()
            f.receiver.decoder.createCodec = { format ->
                attempted.add(format)
                check(format.width <= 640 && format.height <= 400) { "Panel-sized format unsupported" }
                MediaCodec.createDecoderByType(format.mimeType)
            }
            val surface = Surface(SurfaceTexture(1))
            try {
                assertTrue("T466: negotiated scaled format must configure, attempted=$attempted", f.receiver.setupCodec(surface))
                assertEquals(listOf(DecoderFormat("video/avc", 640, 400, 30)), attempted)
            } finally { f.receiver.stop(); surface.release() }
        }
    }

    @Test fun t466_liveFormatChangeRestartsOnceAndIdenticalFormatKeepsSession() {
        val receiver = VideoReceiver { error("T466: must wait for Surface") }
        val format = DecoderFormat("video/avc", 640, 400, 30)
        var disconnected = 0
        receiver.onDisconnected = { disconnected++ }
        try {
            receiver.start()
            receiver.setStreamFormat(format)
            assertEquals(1, disconnected)
            assertEquals(Triple(640, 400, 30), Triple(receiver.formatWidth, receiver.formatHeight, receiver.streamFps))
            receiver.setStreamFormat(format)
            assertEquals("T466: unchanged format restarted video", 1, disconnected)
        } finally { receiver.stop() }
    }

    @Test fun t466_resizeAndReconnectRetireOldFormatWithLegacyFallback() {
        Fixture().use { f ->
            f.greet()
            f.greet(1280, 800, 60)
            assertEquals(1280, f.receiver.formatWidth)
            assertEquals(800, f.receiver.formatHeight)
            val old = f.sockets.last()
            f.session.stop(); f.session.start()
            f.sockets.last().greet("""{"codec":"h264","pen_only":true}""")
            old.greet("""{"codec":"hevc","fps":90,"video_width":2000,"video_height":1000}""")
            assertEquals("T466: legacy host uses native fallback, not previous connection format", 3840, f.receiver.formatWidth)
            assertEquals(2160, f.receiver.formatHeight)
            assertEquals("video/avc", f.receiver.mimeType)
        }
    }

    @Test fun t466_queuedRetiredFormatAndMalformedDimensionsCannotReachDecoder() {
        val queued = mutableListOf<() -> Unit>()
        Fixture { queued.add(it) }.use { f ->
            f.greet()
            f.session.stop(); f.session.start()
            queued.toList().forEach { it() }; queued.clear()
            assertNotEquals("T466: obsolete queued format applied", 640, f.receiver.formatWidth)
            f.greet(1280, 800, 60)
            queued.toList().forEach { it() }; queued.clear()
            f.greet(-1, 800, 60)
            queued.toList().forEach { it() }
            assertEquals(1280, f.receiver.formatWidth)
            assertEquals(800, f.receiver.formatHeight)
        }
    }
}
