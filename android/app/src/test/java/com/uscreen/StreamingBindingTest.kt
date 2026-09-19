package com.uscreen

import android.content.ComponentName
import android.content.ContextWrapper
import android.content.Intent
import android.os.Looper
import kotlinx.coroutines.flow.MutableStateFlow
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34])
class StreamingBindingTest {
    private class ServiceContext : ContextWrapper(RuntimeEnvironment.getApplication()) {
        val commands = mutableListOf<Intent>()
        var starts = 0; var stops = 0; var rejectStart = false; var rejectUpdate = false
        override fun startForegroundService(intent: Intent): ComponentName? {
            starts++
            if (rejectStart) throw IllegalStateException("T388 background start denied")
            commands.add(intent)
            return ComponentName(this, StreamingService::class.java)
        }
        override fun startService(intent: Intent): ComponentName? {
            if (rejectUpdate) throw SecurityException("T388 update denied")
            commands.add(intent)
            return ComponentName(this, StreamingService::class.java)
        }
        override fun stopService(intent: Intent): Boolean { stops++; return true }
    }
    private fun idle() = shadowOf(Looper.getMainLooper()).idle()

    @Test fun t497_completedPolicyFlowKeepsTheServiceOwnedUntilStop() {
        val context = ServiceContext()
        val binding = StreamingPowerBinding(context)
        val initial = StreamingPower(false)
        binding.start(initial, kotlinx.coroutines.flow.flowOf(initial, StreamingPower(true)))
        idle()
        assertEquals(3, context.commands.size)
        assertTrue(context.commands.last().getBooleanExtra("battery_saver", false))
        assertEquals(0, context.stops)
        binding.stop()
        assertEquals(1, context.stops)
    }

    @Test fun t388_stop_cancels_updates_and_restart_takes_current_policy() {
        val context = ServiceContext()
        val binding = StreamingPowerBinding(context)
        val updates = MutableStateFlow(StreamingPower(true, true, StreamTransport.USB))
        binding.start(updates.value, updates); binding.start(updates.value, updates); idle()
        assertEquals(1, context.starts)
        assertTrue(context.commands.all { it.getBooleanExtra("battery_saver", false) })
        binding.stop()
        val count = context.commands.size
        updates.value = StreamingPower(false, true, StreamTransport.NETWORK); idle()
        assertEquals("T388: retired Activity restarted the service", count, context.commands.size)
        binding.start(updates.value, updates); idle()
        assertEquals(2, context.starts)
        assertFalse(context.commands.last().getBooleanExtra("battery_saver", true))
        binding.stop(); binding.stop(); idle()
        assertEquals(2, context.stops)
    }

    @Test fun t388_rejected_start_and_update_do_not_crash_or_keep_observing() {
        val context = ServiceContext().apply { rejectStart = true }
        val binding = StreamingPowerBinding(context)
        val updates = MutableStateFlow(StreamingPower(true))
        binding.start(updates.value, updates); idle()
        assertTrue(context.commands.isEmpty())
        updates.value = StreamingPower(true, true); idle()
        assertTrue(context.commands.isEmpty())
        context.rejectStart = false
        binding.start(updates.value, updates); idle()
        context.rejectUpdate = true
        updates.value = StreamingPower(true, true, StreamTransport.USB); idle()
        assertEquals(1, context.stops)
        val count = context.commands.size
        updates.value = StreamingPower(false); idle()
        assertEquals(count, context.commands.size)
        binding.stop()
    }
}
