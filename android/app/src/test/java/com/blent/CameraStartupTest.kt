// Copyright (c) 2026 Geraldo Netto
package com.blent

import java.util.concurrent.*
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicReference
import kotlinx.coroutines.*
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34])
class CameraStartupTest {
    @Test fun t707_lateCameraCallbackRetiresOffCallerBeforeCleanupCompletes() {
        val resources = CameraResources()
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
        val thread = AtomicReference<Thread>()
        val closes = AtomicInteger()
        lateinit var startup: CameraStartup<Int>
        val job = scope.launch { suspendCancellableCoroutine<Int> { continuation ->
            startup = CameraStartup(resources, continuation) { thread.set(Thread.currentThread()); closes.incrementAndGet() }
        } }
        val executor = Executors.newSingleThreadExecutor()
        try {
            job.cancel()
            resources.cancel()
            val began = CountDownLatch(1)
            val retiring = executor.submit { began.countDown(); resources.close() }
            assertTrue(began.await(2, TimeUnit.SECONDS))
            assertThrows("T707 retirement forgot outstanding startup", TimeoutException::class.java) {
                retiring.get(100, TimeUnit.MILLISECONDS)
            }
            startup.opened(1)
            retiring.get(2, TimeUnit.SECONDS)
            assertNotSame(Thread.currentThread(), thread.get())
            assertEquals(1, closes.get())
            startup.failed(1, "late disconnect")
            startup.abort(IllegalStateException("duplicate terminal callback"))
            resources.close()
            assertEquals(1, closes.get())
        } finally { executor.shutdownNow(); scope.cancel() }
    }

    @Test fun t707_synchronousStartupFailureReleasesLeaseAndPreservesFailure() = runBlocking {
        for (failed in listOf(false, true)) {
            val resources = CameraResources()
            var closes = 0
            val failure = runCatching {
                suspendCancellableCoroutine<Int> { continuation ->
                    val startup = CameraStartup(resources, continuation) { closes++ }
                    if (failed) startup.failed(1, "native failure")
                    else startup.abort(IllegalStateException("native failure"))
                }
            }.exceptionOrNull()
            assertEquals("native failure", failure?.message)
            resources.close()
            assertEquals(if (failed) 1 else 0, closes)
        }
    }

    @Test fun t707_disconnectAfterOpenRetainsFailureAndClosesOnlyOnce() = runBlocking {
        val resources = CameraResources()
        var closes = 0
        lateinit var startup: CameraStartup<Int>
        val result = suspendCancellableCoroutine<Int> { continuation ->
            startup = CameraStartup(resources, continuation) { closes++ }
            startup.opened(7)
        }
        assertEquals(7, result)
        startup.failed(7, "Camera disconnected")
        assertEquals("Camera disconnected", runCatching { resources.checkActive() }.exceptionOrNull()?.message)
        resources.close()
        assertEquals(1, closes)
    }

    @Test fun t707_cancelInterruptsTransportOnceAndRejectsNewAdmission() {
        val resources = CameraResources()
        var interruptions = 0
        resources.checkActive()
        val release = resources.startup()
        resources.interrupt { interruptions++; error("socket close failed") }
        resources.cancel(); resources.cancel()
        assertEquals(1, interruptions)
        assertTrue(runCatching { resources.checkActive() }.exceptionOrNull() is kotlinx.coroutines.CancellationException)
        assertTrue(runCatching { resources.startup() }.exceptionOrNull() is kotlinx.coroutines.CancellationException)
        assertTrue(runCatching { resources.interrupt { interruptions++ } }.exceptionOrNull() is kotlinx.coroutines.CancellationException)
        assertEquals(2, interruptions)
        release(); release()
        resources.close()
    }

    @Test fun t707_concurrentClosersWaitForTheSameNativeDestruction() {
        val resources = CameraResources()
        val entered = CountDownLatch(1)
        val release = CountDownLatch(1)
        val closes = AtomicInteger()
        val executor = Executors.newFixedThreadPool(2)
        resources.own { entered.countDown(); release.await(2, TimeUnit.SECONDS); closes.incrementAndGet() }
        try {
            val first = executor.submit { resources.close() }
            assertTrue(entered.await(1, TimeUnit.SECONDS))
            val began = CountDownLatch(1)
            val second = executor.submit { began.countDown(); resources.close() }
            assertTrue(began.await(1, TimeUnit.SECONDS))
            assertFalse(first.isDone)
            assertThrows(TimeoutException::class.java) { second.get(100, TimeUnit.MILLISECONDS) }
            release.countDown()
            first.get(2, TimeUnit.SECONDS); second.get(2, TimeUnit.SECONDS)
            assertEquals(1, closes.get())
        } finally { release.countDown(); executor.shutdownNow() }
    }
}
