package com.uscreen

import com.uscreen.benchmark.SocketReplay
import java.util.concurrent.FutureTask
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34], shadows = [DirectInputCodecShadow::class])
class SocketReplayTest {
    @get:org.junit.Rule val decoderInventory = DecoderInventoryRule()
    @Test fun t403_backgrounding_cancels_a_backpressured_producer() = stalledWrite(true)

    @Test fun t403_write_deadline_bounds_a_backpressured_producer() = stalledWrite(false)

    private fun stalledWrite(background: Boolean) {
        DirectInputFixture().use { fixture ->
            val active = AtomicBoolean(true)
            val entered = CountDownLatch(1)
            val resume = CountDownLatch(1)
            DirectInputCodecShadow.beforeInput = { entered.countDown(); check(resume.await(3, TimeUnit.SECONDS)) }
            val timeout = if (background) 10_000L else 100L
            SocketReplay(fixture.decoder, true, active::get, timeout).use { replay ->
                val writer = FutureTask {
                    try { replay.feed(ByteArray(8 * 1024 * 1024 - 4), false, 1); null }
                    catch (error: Exception) { error }
                }
                val thread = Thread(writer).apply { start() }
                try {
                    assertTrue(entered.await(1, TimeUnit.SECONDS))
                    if (background) active.set(false)
                    val failure = writer.get(1, TimeUnit.SECONDS)
                    assertNotNull("T403: native decoder progress must not control write cancellation", failure)
                    if (background) assertTrue(failure is java.io.InterruptedIOException)
                    else assertTrue(failure is java.net.SocketTimeoutException)
                } finally { resume.countDown(); replay.close(); thread.join(1_000) }
            }
        }
    }

    @Test fun t403_rejected_input_unblocks_the_socket_producer() {
        DirectInputFixture().use { fixture ->
            SocketReplay(fixture.decoder, true).use { replay ->
                val writer = FutureTask {
                    try { replay.feed(ByteArray(8 * 1024 * 1024 - 4), false, 1); null }
                    catch (error: Exception) { error }
                }
                val thread = Thread(writer).apply { start() }
                try {
                    assertNotNull("T403: rejected input must fail the producer write", writer.get(1, TimeUnit.SECONDS))
                    assertTrue(DirectInputCodecShadow.queued.isEmpty())
                } finally { replay.close(); thread.join(1_000) }
            }
        }
    }
}
