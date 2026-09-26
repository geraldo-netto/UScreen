package com.blent

import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34])
class AllocationProfileTest {
    class WindowOnlyActivity : AllocationProfileActivity() {
        override fun launchWorkload() = Unit
    }

    @Test fun t595_profileKeepsScreenOnAcrossActivityRecreation() {
        val controller = org.robolectric.Robolectric.buildActivity(WindowOnlyActivity::class.java).setup()
        try {
            val flag = android.view.WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON
            assertTrue(controller.get().window.attributes.flags and flag != 0)
            controller.recreate()
            assertTrue(controller.get().window.attributes.flags and flag != 0)
        } finally { controller.pause().stop().destroy() }
    }

    @Test fun t589_fixtureRepeatsLengthsAcrossGrowthAndReconnect() {
        val source = PacketStream(intArrayOf(65536, 2 * 1024 * 1024, VideoReceiver.MAX_FRAME_SIZE + 1))
        repeat(3) {
            val reader = VideoPacketReader(source)
            for (size in intArrayOf(65536, 2 * 1024 * 1024, VideoReceiver.MAX_FRAME_SIZE + 1)) {
                assertTrue(reader.read())
                assertEquals(size, reader.size)
            }
        }
    }
}
