package com.blent.benchmark

import android.graphics.SurfaceTexture
import android.media.MediaCodec
import android.view.Surface
import com.blent.*
import java.io.DataInputStream
import java.io.DataOutputStream
import java.net.ServerSocket
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

// The generated APK bridge binds the same production format/receipt APIs.
internal fun requestFormat(mime: String, width: Int, height: Int, fps: Int, selection: JSONObject?) =
    DecoderFormat(mime, width, height, fps, selection = selection?.let(DecoderSelection::read))
internal fun replayReceipt(decoder: DecoderSession) = decoder.configuredSelectionReceipt

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34], shadows = [WatchdogCodecShadow::class])
class UsbReplayTest {
    @get:org.junit.Rule val inventory = DecoderInventoryRule()

    @Test fun t479_controlledRestartKeepsReplayActiveAndRechecksReceipt() {
        val surface = Surface(SurfaceTexture(1))
        val replay = UsbReplay(surface, AtomicBoolean(true))
        val decoder = UsbReplay::class.java.getDeclaredField("decoder").apply { isAccessible = true }.get(replay) as DecoderSession
        decoder.createCodec = { MediaCodec.createDecoderByType(it.mimeType) }
        val worker = Executors.newSingleThreadExecutor()
        try {
            ServerSocket(0).use { server ->
                server.soTimeout = 3000
                val task = worker.submit<JSONObject> { replay.run(server.localPort) }
                server.accept().use { socket ->
                    socket.soTimeout = 3000
                    val input = DataInputStream(socket.getInputStream())
                    val output = DataOutputStream(socket.getOutputStream())
                    output.writeUTF(metadata().toString())
                    assertReady(input)
                    output.writeInt(1) // Controlled decoder retirement, not failed rendering.
                    assertReady(input)
                    output.writeInt(2)
                    val result = task.get(3, TimeUnit.SECONDS)
                    assertTrue(result.toString(), result.getBoolean("completed"))
                    assertEquals(1, result.getJSONObject("stats").getInt("invalidations"))
                }
            }
        } finally { worker.shutdownNow(); decoder.releaseCodec(); surface.release() }
    }

    private fun field(replay: UsbReplay, name: String): Any =
        UsbReplay::class.java.getDeclaredField(name).apply { isAccessible = true }.get(replay)!!

    @Test fun t571_ackObligationExistsBeforeRenderCountCanBePublished() {
        val surface = Surface(SurfaceTexture(1))
        val replay = UsbReplay(surface, AtomicBoolean(true))
        val stats = field(replay, "stats") as ReplayStats
        val decoder = field(replay, "decoder") as DecoderSession
        val events = DecoderSession::class.java.getDeclaredField("events").apply { isAccessible = true }
            .get(decoder) as DecoderEvents
        val callback = Thread { events.rendered(1, 10) }
        stats.begin(1)
        try {
            synchronized(stats) {
                callback.start()
                val deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(2)
                while (callback.state != Thread.State.BLOCKED && System.nanoTime() < deadline) Thread.yield()
                assertEquals(Thread.State.BLOCKED, callback.state)
                // Completion must never observe the final render before its ACK obligation.
                assertEquals(1, (field(replay, "pending") as java.util.concurrent.atomic.AtomicInteger).get())
            }
        } finally { callback.join(2000); surface.release() }
        assertFalse(callback.isAlive)
        assertEquals(1, stats.count())
    }

    @Test fun t571_deadlineCannotReportMissingRenderOrUndrainedAckAsComplete() {
        assertIncomplete(true)
        assertIncomplete(false)
    }

    private fun assertIncomplete(missingRender: Boolean) {
        val surface = Surface(SurfaceTexture(1))
        val replay = UsbReplay(surface, AtomicBoolean(true))
        val decoder = field(replay, "decoder") as DecoderSession
        decoder.createCodec = { MediaCodec.createDecoderByType(it.mimeType) }
        if (missingRender) {
            UsbReplay::class.java.getDeclaredField("count").apply { isAccessible = true }.setInt(replay, 1)
        } else {
            (field(replay, "pending") as java.util.concurrent.atomic.AtomicInteger).set(1)
        }
        val worker = Executors.newSingleThreadExecutor()
        try {
            ServerSocket(0).use { server ->
                server.soTimeout = 3000
                val task = worker.submit<JSONObject> { replay.run(server.localPort) }
                server.accept().use { socket ->
                    socket.soTimeout = 3000
                    val input = DataInputStream(socket.getInputStream())
                    val output = DataOutputStream(socket.getOutputStream())
                    output.writeUTF(metadata().toString())
                    assertReady(input)
                    output.writeInt(2)
                    val result = task.get(3, TimeUnit.SECONDS)
                    assertFalse(result.toString(), result.getBoolean("completed"))
                }
            }
        } finally { worker.shutdownNow(); decoder.releaseCodec(); surface.release() }
    }

    private fun metadata(): JSONObject {
        val request = JSONObject(javaClass.getResource("/decoder-selection.json")!!.readText())
        return JSONObject().put("width", 1280).put("height", 800).put("fps", 60)
            .put("mime", "video/avc").put("selection", request)
    }

    private fun assertReady(input: DataInputStream) {
        assertEquals(0, input.readByte().toInt())
        assertEquals("10:vendor.avc:h264:baseline:41:8:0:120", input.readUTF())
        assertTrue(input.readLong() >= 0)
    }
}
