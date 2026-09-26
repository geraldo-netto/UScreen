package com.blent

import android.media.MediaCodec
import android.media.MediaFormat
import android.os.Build
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34])
class DecoderConfigurationTest {
    private class Support(private val low: Boolean, private val rate: Boolean) : DecoderConfiguration.Support {
        var queries = 0
        override fun lowLatency(codec: MediaCodec, mime: String): Boolean { queries++; assertEquals("video/avc", mime); return low }
        override fun operatingRate(codec: MediaCodec, parameters: DecoderFormat, fps: Int): Boolean {
            queries++; assertEquals(1280, parameters.width); assertEquals(800, parameters.height)
            assertTrue(fps in setOf(60, 120)); return rate
        }
    }

    @Test fun t386_supportedProfileUsesActualFeaturesAndRateHeadroom() {
        val codec = MediaCodec.createDecoderByType("video/avc")
        try {
            for (low in listOf(false, true)) for (rate in listOf(false, true)) {
                val support = Support(low, rate)
                val format = DecoderConfiguration.format(codec, DecoderFormat("video/avc", 1280, 800, 60),
                    DecoderProfile(true, DecoderHints.SUPPORTED, 2), true, support)
                assertEquals(low && Build.VERSION.SDK_INT >= 30, format.containsKey("low-latency"))
                assertEquals(rate, format.containsKey(MediaFormat.KEY_OPERATING_RATE))
                if (rate) assertEquals(120, format.getInteger(MediaFormat.KEY_OPERATING_RATE))
                assertFalse(format.containsKey("vendor.qti-ext-dec-low-latency.enable"))
                assertEquals(60, format.getInteger(MediaFormat.KEY_FRAME_RATE))
                assertEquals(MediaFormat.COLOR_STANDARD_BT709, format.getInteger(MediaFormat.KEY_COLOR_STANDARD))
            }
        } finally { codec.release() }
    }

    @Test fun t386_fallbackAndUnhintedProfilesNeverProbeOrEnablePerformanceHints() {
        val codec = MediaCodec.createDecoderByType("video/avc")
        val support = Support(true, true)
        val parameters = DecoderFormat("video/avc", 1280, 800, 60)
        try {
            val profiles = listOf(DecoderProfile(), DecoderProfile(true, DecoderHints.SUPPORTED), DecoderProfile(true, DecoderHints.NONE))
            for (profile in profiles) {
                val format = DecoderConfiguration.format(codec, parameters, profile, false, support)
                assertFalse(format.containsKey("low-latency"))
                assertFalse(format.containsKey(MediaFormat.KEY_OPERATING_RATE))
                assertFalse(format.containsKey("vendor.qti-ext-dec-low-latency.enable"))
            }
            val unhinted = DecoderConfiguration.format(codec, parameters, DecoderProfile(true, DecoderHints.NONE), true, support)
            assertFalse(unhinted.containsKey(MediaFormat.KEY_OPERATING_RATE))
            assertEquals(0, support.queries)
            val legacy = DecoderConfiguration.format(codec, parameters, DecoderProfile(), true, support)
            assertEquals(120, legacy.getInteger(MediaFormat.KEY_OPERATING_RATE))
            assertEquals(1, legacy.getInteger("vendor.qti-ext-dec-low-latency.enable"))
            assertEquals(0, support.queries)
        } finally { codec.release() }
    }
}
