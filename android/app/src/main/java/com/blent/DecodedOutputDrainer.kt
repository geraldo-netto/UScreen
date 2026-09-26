package com.blent

import android.media.MediaCodec

/** Experimental decode-all/render-latest policy. Never drops compressed input.
 * Called inside one codec lifetime borrow, with no receiver monitor held. */
internal class DecodedOutputDrainer(
    private val codec: MediaCodec,
    private val alive: () -> Boolean,
    private val discarded: (Int) -> Unit,
) {
    private val nextInfo = MediaCodec.BufferInfo()

    fun release(firstIndex: Int, firstSequence: Int, latest: Boolean): Int? {
        var index = firstIndex
        var sequence = firstSequence
        if (latest) repeat(3) { // At most four outputs per batch: no starvation.
            if (!alive()) return null
            val next = codec.dequeueOutputBuffer(nextInfo, 0)
            if (next < 0) return present(index, sequence)
            if (!alive()) return null
            codec.releaseOutputBuffer(index, false)
            discarded(sequence)
            index = next
            sequence = nextInfo.presentationTimeUs.toInt()
        }
        return present(index, sequence)
    }

    private fun present(index: Int, sequence: Int): Int? {
        if (!alive()) return null
        codec.releaseOutputBuffer(index, true)
        return sequence
    }
}
