package com.uscreen

import android.graphics.SurfaceTexture
import android.media.MediaCodec
import android.media.MediaCrypto
import android.media.MediaFormat
import android.os.Handler
import android.view.Surface
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicReference
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.Implementation
import org.robolectric.annotation.Implements
import org.robolectric.shadows.ShadowMediaCodec

@Suppress("UNUSED_PARAMETER")
@Implements(MediaCodec::class)
class StartupCodecShadow : ShadowMediaCodec() {
    companion object {
        var stage = ""
        var entered = CountDownLatch(1)
        var resume = CountDownLatch(1)
        var released = CountDownLatch(1)
        fun gate(at: String) {
            if (stage != at) return
            entered.countDown()
            check(resume.await(5, TimeUnit.SECONDS))
        }
    }
    @Implementation fun configure(format: MediaFormat, surface: Surface?, crypto: MediaCrypto?, flags: Int) { gate("configure") }
    @Implementation fun setVideoScalingMode(mode: Int) {}
    @Implementation fun setOnFrameRenderedListener(listener: MediaCodec.OnFrameRenderedListener, handler: Handler) {}
    @Implementation fun start() { gate("start") }
    @Implementation fun stop() {}
    @Implementation fun release() { released.countDown() }
    @Implementation fun dequeueOutputBuffer(info: MediaCodec.BufferInfo, timeoutUs: Long): Int {
        Thread.sleep(10)
        return -1
    }
}

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34], shadows = [StartupCodecShadow::class])
class DecoderStartupTest {
    @Test fun t425_stopDuringNativeCreation() = blockedSetup("create", false)
    @Test fun t425_stopDuringNativeConfiguration() = blockedSetup("configure", false)
    @Test fun t425_stopDuringNativeStart() = blockedSetup("start", false)
    @Test fun t425_surfaceDestructionDuringNativeConfiguration() = blockedSetup("configure", true)

    @Test fun t425_surfaceCallbackDefersNativeSetup() {
        val surface = Surface(SurfaceTexture(1))
        val view = object : android.view.SurfaceView(androidx.test.core.app.ApplicationProvider.getApplicationContext()) {
            override fun getHolder(): android.view.SurfaceHolder = object : android.view.SurfaceHolder by super.getHolder() {
                override fun getSurface() = surface
            }
        }
        assertTrue("T425: fixture must exercise the ready-Surface path", view.holder.surface.isValid)
        val receiver = VideoReceiver()
        VideoReceiver::class.java.getDeclaredField("isRunning").apply { isAccessible = true }.set(receiver, true)
        val allocations = AtomicInteger()
        receiver.decoder.createCodec = { allocations.incrementAndGet(); throw IllegalStateException("native UI setup") }
        try {
            receiver.setSurface(view)
            assertEquals("T425: Activity Surface callback entered native setup", 0, allocations.get())
        } finally { receiver.stop(); surface.release() }
    }

    private fun blockedSetup(stage: String, destroySurface: Boolean) {
        StartupCodecShadow.stage = stage
        StartupCodecShadow.entered = CountDownLatch(1)
        StartupCodecShadow.resume = CountDownLatch(1)
        StartupCodecShadow.released = CountDownLatch(1)
        val surface = Surface(SurfaceTexture(1))
        val receiver = VideoReceiver()
        val result = AtomicReference<Boolean?>()
        val error = AtomicReference<Throwable?>()
        receiver.decoder.createCodec = { mime ->
            StartupCodecShadow.gate("create")
            MediaCodec.createDecoderByType(mime)
        }
        val setup = Thread {
            try { result.set(receiver.setupCodec(surface)) }
            catch (failure: Throwable) { error.set(failure) }
        }.apply { start() }
        val stopped = CountDownLatch(1)
        val stop = Thread {
            if (destroySurface) receiver.onSurfaceDestroyed() else receiver.stop()
            stopped.countDown()
        }
        try {
            assertTrue("T425: native setup was not reached", StartupCodecShadow.entered.await(2, TimeUnit.SECONDS))
            stop.start()
            assertTrue("T425: native setup blocked lifecycle shutdown", stopped.await(300, TimeUnit.MILLISECONDS))
            assertNull(receiver.decoder.mediaCodec)
            recreatedReceiverCannotAllocate(surface)
        } finally {
            StartupCodecShadow.resume.countDown()
            setup.join(2000)
            stop.join(2000)
            receiver.stop()
            surface.release()
        }
        error.get()?.let { throw AssertionError("T425: setup worker", it) }
        assertFalse("T425: setup did not retire", setup.isAlive)
        assertEquals("T425: late setup published an obsolete codec", false, result.get())
        assertNull(receiver.decoder.mediaCodec)
        assertTrue("T425: late codec leaked", StartupCodecShadow.released.await(2, TimeUnit.SECONDS))
    }

    private fun recreatedReceiverCannotAllocate(surface: Surface) {
        val allocations = AtomicInteger()
        repeat(3) {
            val recreated = VideoReceiver()
            recreated.decoder.createCodec = {
                allocations.incrementAndGet()
                throw IllegalStateException("T425: replacement bypassed setup ownership")
            }
            try { assertFalse(recreated.setupCodec(surface)) }
            finally { recreated.stop() }
        }
        assertEquals("T425: Activity recreation admitted repeated native allocations", 0, allocations.get())
    }
}
