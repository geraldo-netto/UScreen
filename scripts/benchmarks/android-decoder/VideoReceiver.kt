package com.uscreen

/** Constants needed by the copied production decoder/timing sources. This
 * experiment has no network/control service and cannot create a desktop. */
class VideoReceiver {
    companion object {
        const val TAG = "UScreenDecoderBench"
        const val ACK_EVERY = 1
        const val ARRIVAL_RING = 64
        const val MAX_FRAME_SIZE = 8 * 1024 * 1024
    }
}
