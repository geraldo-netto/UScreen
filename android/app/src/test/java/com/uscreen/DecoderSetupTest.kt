package com.uscreen

import android.media.MediaCodec
import android.media.MediaCrypto
import android.media.MediaFormat
import android.os.Handler
import android.os.HandlerThread
import android.view.Surface
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
class FailingCodecShadow : ShadowMediaCodec() {
    companion object {
        var failAt = "configure"
        var releases = 0
        var lastFormat: MediaFormat? = null
    }
    @Implementation fun configure(format: MediaFormat, surface: Surface?, crypto: MediaCrypto?, flags: Int) {
        lastFormat = format
        if (failAt == "configure") throw IllegalStateException("injected configure failure")
    }
    @Implementation fun setVideoScalingMode(mode: Int) {}
    @Implementation fun setOnFrameRenderedListener(listener: MediaCodec.OnFrameRenderedListener, handler: Handler) {
        if (failAt == "listener") throw IllegalStateException("injected listener failure")
    }
    @Implementation fun start() {
        if (failAt == "start") throw IllegalStateException("injected start failure")
    }
    @Implementation fun stop() {}
    @Implementation fun release() { releases++ }
}

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34], shadows = [FailingCodecShadow::class])
class DecoderSetupTest {
    @Test fun t120_decoderHintsUseTheEffectiveRateAcrossRestarts() {
        val receiver = VideoReceiver()
        val setup = VideoReceiver::class.java.getDeclaredMethod("setupCodec", Surface::class.java).apply { isAccessible = true }
        val surface = Surface(android.graphics.SurfaceTexture(1))
        FailingCodecShadow.failAt = "start"
        try {
            for (fps in listOf(30, 90, 60)) {
                receiver.streamFps = fps
                assertEquals(false, setup.invoke(receiver, surface))
                assertEquals(fps, FailingCodecShadow.lastFormat!!.getInteger(MediaFormat.KEY_FRAME_RATE))
                assertEquals(fps * 2, FailingCodecShadow.lastFormat!!.getInteger("operating-rate"))
                receiver.stop()
            }
        } finally { receiver.stop(); surface.release() }
    }

    @Test fun t089_configureFailureReleasesResources() = checkFailure("configure")
    @Test fun t089_listenerFailureReleasesResources() = checkFailure("listener")
    @Test fun t089_startFailureReleasesResources() = checkFailure("start")

    private fun checkFailure(stage: String) {
        FailingCodecShadow.failAt = stage
        FailingCodecShadow.releases = 0
        val threads = mutableListOf<HandlerThread>()
        var quits = 0
        val receiver = VideoReceiver()
        receiver.callbackThreadFactory = {
            object : HandlerThread("uscreen-test-frame-cb") {
                override fun quitSafely(): Boolean { quits++; return super.quitSafely() }
            }.also { threads.add(it) }
        }
        val setup = VideoReceiver::class.java.getDeclaredMethod("setupCodec", Surface::class.java).apply { isAccessible = true }
        val surface = Surface(android.graphics.SurfaceTexture(1))
        try {
            for (attempt in 1..2) {
                assertEquals(false, setup.invoke(receiver, surface))
                assertEquals("$stage leaked codec on attempt $attempt", attempt, FailingCodecShadow.releases)
                assertEquals(threads.size, quits)
                threads.forEach { it.join(500); assertFalse("Callback thread leaked", it.isAlive) }
            }
            receiver.stop()
            assertEquals("Failed resources must not be released twice", 2, FailingCodecShadow.releases)
            assertEquals(threads.size, quits)
        } finally {
            receiver.stop()
            threads.filter { it.isAlive }.forEach { it.quitSafely(); it.join(500) }
            surface.release()
        }
    }
}
