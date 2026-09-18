package com.uscreen

import android.media.MediaCodec
import java.util.ArrayDeque
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.Implementation
import org.robolectric.annotation.Implements
import org.robolectric.shadows.ShadowMediaCodec

@Implements(MediaCodec::class)
class DrainCodecShadow : ShadowMediaCodec() {
    companion object {
        val pending = ArrayDeque<Int>()
        val releases = mutableListOf<Pair<Int, Boolean>>()
        var dequeues = 0
        var afterDequeue: () -> Unit = {}
    }
    @Implementation fun dequeueOutputBuffer(info: MediaCodec.BufferInfo, timeoutUs: Long): Int {
        assertEquals("T399: lookahead never waits for another picture", 0L, timeoutUs)
        dequeues++
        val index = pending.pollFirst() ?: -1
        info.presentationTimeUs = (index + 100).toLong()
        afterDequeue()
        return index
    }
    @Implementation override fun releaseOutputBuffer(index: Int, render: Boolean) { releases.add(index to render) }
}

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34], shadows = [DrainCodecShadow::class])
class DecodedOutputDrainerTest {
    private fun exercise(latest: Boolean, pending: List<Int>, retireDuringRead: Boolean = false): Pair<Int?, List<Int>> {
        DrainCodecShadow.pending.clear(); DrainCodecShadow.pending.addAll(pending)
        DrainCodecShadow.releases.clear(); DrainCodecShadow.dequeues = 0
        var alive = true
        DrainCodecShadow.afterDequeue = { if (retireDuringRead) alive = false }
        val codec = MediaCodec.createDecoderByType("video/avc")
        val dropped = mutableListOf<Int>()
        try {
            val result = DecodedOutputDrainer(codec, { alive }, dropped::add).release(0, 100, latest)
            return result to dropped
        } finally { codec.release() }
    }

    @Test fun t399_default_releases_every_output_without_lookahead() {
        assertEquals(100 to emptyList<Int>(), exercise(false, listOf(1, 2)))
        assertEquals(listOf(0 to true), DrainCodecShadow.releases)
        assertEquals(0, DrainCodecShadow.dequeues)
    }
    @Test fun t399_latest_preserves_decoding_and_presents_only_last_ready_output() {
        assertEquals(102 to listOf(100, 101), exercise(true, listOf(1, 2)))
        assertEquals(listOf(0 to false, 1 to false, 2 to true), DrainCodecShadow.releases)
    }
    @Test fun t399_continuous_output_cannot_starve_presentation() {
        assertEquals(103 to listOf(100, 101, 102), exercise(true, (1..8).toList()))
        assertEquals(3, DrainCodecShadow.dequeues)
        assertEquals(3 to true, DrainCodecShadow.releases.last())
    }
    @Test fun t399_sparse_or_format_change_preserves_the_available_frame() {
        for (pending in listOf(emptyList(), listOf(MediaCodec.INFO_OUTPUT_FORMAT_CHANGED))) {
            assertEquals(100 to emptyList<Int>(), exercise(true, pending))
            assertEquals(listOf(0 to true), DrainCodecShadow.releases)
        }
    }
    @Test fun t399_retirement_during_lookahead_cannot_present_an_old_surface_frame() {
        assertEquals(null to emptyList<Int>(), exercise(true, listOf(1), true))
        assertTrue(DrainCodecShadow.releases.isEmpty())
    }
}
