// Copyright (c) 2026 Geraldo Netto
package com.blent

import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicReference
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.MutableStateFlow
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34])
class CameraRetirementTest {
    private class Fixture(capture: suspend (CameraEndpoint, CameraLens, Int, CameraResources) -> Unit) : AutoCloseable {
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
        val commands = MutableStateFlow<CameraEndpoint?>(null)
        val binding = CameraBinding(RuntimeEnvironment.getApplication(), { 0 }, {}, { true }, capture, commands, scope)
        init {
            binding.start()
            commands.value = CameraEndpoint("a".repeat(64), 12345, 1280, 720, 30, 3000, CameraLens.FRONT)
        }
        override fun close() { binding.shutdown(); scope.cancel() }
    }

    @Test fun t707_blockedNativeRetirementNeverBlocksStopOrStartsReplacement() {
        val entered = CountDownLatch(1)
        val release = CountDownLatch(1)
        val closed = AtomicInteger()
        val opened = AtomicInteger()
        val closingThread = AtomicReference<Thread>()
        val caller = Thread.currentThread()
        val fixture = Fixture { _, _, _, resources ->
            val number = opened.incrementAndGet()
            resources.own {
                closingThread.set(Thread.currentThread())
                entered.countDown()
                if (number == 1) release.await(3, TimeUnit.SECONDS)
                closed.incrementAndGet()
            }
            awaitCancellation()
        }
        try {
            fixture.binding.choose(null)
            assertTrue(entered.await(2, TimeUnit.SECONDS))
            assertNotSame("T707 native destruction ran on the Activity caller", caller, closingThread.get())
            assertEquals("T707 Stop waited for native destruction", 0, closed.get())
            fixture.binding.choose(CameraLens.REAR)
            assertEquals("T707 replacement started before retirement", 1, opened.get())
            release.countDown()
            awaitValue(opened, 2)
            fixture.binding.choose(null)
            awaitValue(closed, 2)
        } finally { release.countDown(); fixture.close() }
    }

    @Test fun t707_rapidReplacementsWaitForBorrowAndLateStartupCleanup() {
        val borrowing = CountDownLatch(1)
        val release = CountDownLatch(1)
        val opened = AtomicInteger()
        val closed = AtomicInteger()
        val lateClosed = AtomicInteger()
        val fixture = Fixture { _, _, _, resources ->
            val number = opened.incrementAndGet()
            resources.own { closed.incrementAndGet() }
            if (number == 1) withContext(Dispatchers.IO) {
                borrowing.countDown()
                release.await(3, TimeUnit.SECONDS)
                resources.own { lateClosed.incrementAndGet() }
            }
            awaitCancellation()
        }
        try {
            assertTrue(borrowing.await(2, TimeUnit.SECONDS))
            fixture.binding.choose(CameraLens.REAR)
            fixture.binding.choose(CameraLens.FRONT)
            assertEquals("T707 destroyed a borrowed native resource", 0, closed.get())
            assertEquals("T707 cancelled middle worker bypassed the first", 1, opened.get())
            release.countDown()
            awaitValue(opened, 2)
            assertEquals(1, closed.get())
            assertEquals("T707 late startup must retire before replacement", 1, lateClosed.get())
            fixture.binding.choose(null)
            awaitValue(closed, 2)
        } finally { release.countDown(); fixture.close() }
    }

    @Test fun t707_serviceStopFailureCannotSkipNativeCleanup() {
        val closed = CountDownLatch(1)
        val failed = CountDownLatch(1)
        val failure = AtomicReference<Throwable>()
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined + CoroutineExceptionHandler { _, error ->
            failure.set(error); failed.countDown()
        })
        val finish = CompletableDeferred<Unit>()
        val commands = MutableStateFlow<CameraEndpoint?>(null)
        val binding = CameraBinding(RuntimeEnvironment.getApplication(), { 0 }, {}, { true },
            { _, _, _, resources -> resources.own { closed.countDown() }; finish.await() }, commands, scope,
            { run -> if (run == null) error("service stop failed") })
        try {
            binding.start()
            commands.value = CameraEndpoint("a".repeat(64), 12345, 1280, 720, 30, 3000, CameraLens.FRONT, true)
            finish.complete(Unit)
            assertTrue(failed.await(2, TimeUnit.SECONDS))
            assertEquals("service stop failed", failure.get()?.message)
            assertEquals("T707 service failure skipped native destruction", 0L, closed.count)
        } finally { binding.shutdown(); scope.cancel() }
    }

    private fun awaitValue(value: AtomicInteger, expected: Int) {
        val deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(3)
        while (value.get() != expected && System.nanoTime() < deadline) Thread.sleep(5)
        assertEquals(expected, value.get())
    }
}
