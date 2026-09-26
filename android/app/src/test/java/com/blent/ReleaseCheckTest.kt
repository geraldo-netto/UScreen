package com.blent

import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34])
class ReleaseCheckTest {
    private class Pending : ReleaseCall {
        val started = java.util.concurrent.CountDownLatch(1)
        lateinit var result: (String?) -> Unit
        var cancelled = false
        override fun start(result: (String?) -> Unit) { this.result = result; started.countDown() }
        override fun cancel() { cancelled = true }
        fun complete(value: String?) {
            assertTrue(started.await(2, TimeUnit.SECONDS))
            result(value)
        }
    }

    private class Fixture {
        val prefs = Prefs(RuntimeEnvironment.getApplication()).apply { checkUpdates = true }
        val ui = LinkedBlockingQueue<() -> Unit>()
        val calls = LinkedBlockingQueue<Pending>()
        val session = SessionCoordinator(prefs, { ui.add(it) }, null, null, ReleaseChecks {
            Pending().also { call -> calls.add(call) }
        })
        fun start(): Pending {
            session.start()
            session.checkUpdate { "1.0.0" }
            return calls.poll(2, TimeUnit.SECONDS) ?: error("T464: check never started")
        }
        fun drain() { while (true) (ui.poll() ?: return).invoke() }
    }

    @Test fun t464_stoppedActivityRejectsLateCompletionAndCancels() {
        val fixture = Fixture()
        val call = fixture.start()
        fixture.session.stop()
        call.complete("9.0.0")
        fixture.drain()
        assertNull("T464: stopped Activity received an update", fixture.session.updateAvailable)
        assertTrue("T464: outstanding request not cancelled", call.cancelled)
    }

    @Test fun t464_disablingChecksRejectsAlreadyQueuedCompletion() {
        val fixture = Fixture()
        val call = fixture.start()
        call.complete("9.0.0")
        fixture.session.handle(SettingsEvent.CheckUpdates(false))
        fixture.drain()
        assertNull("T464: disabled checks still published an update", fixture.session.updateAvailable)
        assertTrue(call.cancelled)
        fixture.session.stop()
    }

    @Test fun t464_recreationCannotUpdateTheRetiredOwner() {
        val old = Fixture()
        val retired = old.start()
        old.session.stop()
        val fresh = Fixture()
        val current = fresh.start()
        retired.complete("8.0.0")
        current.complete("9.0.0")
        old.drain(); fresh.drain()
        assertNull("T464: recreated Activity retained obsolete completion", old.session.updateAvailable)
        assertEquals("9.0.0", fresh.session.updateAvailable)
        fresh.session.stop()
    }

    @Test fun t464_restartRetriesCancelledCheckAndRejectsItsOldGeneration() {
        val fixture = Fixture()
        val retired = fixture.start()
        fixture.session.stop()
        val current = fixture.start()
        retired.complete("8.0.0")
        fixture.drain()
        assertNull(fixture.session.updateAvailable)
        current.complete("9.0.0")
        fixture.drain()
        assertEquals("9.0.0", fixture.session.updateAvailable)
        fixture.session.stop()
        fixture.session.start()
        fixture.session.checkUpdate { "1.0.0" }
        assertTrue("T464: completed check repeated on resume", fixture.calls.isEmpty())
        fixture.session.stop()
    }

    @Test fun t464_disabledOrStoppedOwnerDoesNotStartRequests() {
        val fixture = Fixture()
        fixture.session.checkUpdate { "1.0.0" }
        assertTrue(fixture.calls.isEmpty())
        fixture.session.handle(SettingsEvent.CheckUpdates(false))
        fixture.session.start()
        assertTrue(fixture.calls.isEmpty())
        fixture.session.handle(SettingsEvent.CheckUpdates(true))
        val call = fixture.calls.poll(2, TimeUnit.SECONDS)!!
        call.complete("9.0.0"); fixture.drain()
        assertEquals("9.0.0", fixture.session.updateAvailable)
        fixture.session.handle(SettingsEvent.CheckUpdates(false))
        assertNull(fixture.session.updateAvailable)
        fixture.session.stop()
    }

    private class Endpoint(private val trickle: Boolean = false) : java.io.Closeable {
        private val server = java.net.ServerSocket(0, 1, java.net.InetAddress.getLoopbackAddress())
        private val accepted = java.util.concurrent.atomic.AtomicReference<java.net.Socket?>()
        val headers = java.util.concurrent.CountDownLatch(1)
        val url = "http://127.0.0.1:${server.localPort}/release"
        private val worker = Thread {
            try { server.accept().use { socket -> serve(socket) } } catch (_: java.io.IOException) { }
        }.apply { start() }

        private fun serve(socket: java.net.Socket) {
            accepted.set(socket)
            socket.soTimeout = 2000
            val input = socket.getInputStream().bufferedReader()
            while (!input.readLine().isNullOrEmpty()) { }
            val body = if (trickle) " ".repeat(1000) else "{\"tag_name\":\"v9.0.0\"}"
            val output = socket.getOutputStream()
            output.write("HTTP/1.1 200 OK\r\nContent-Length: ${body.length}\r\nConnection: close\r\n\r\n".toByteArray())
            output.flush(); headers.countDown()
            for (byte in body.toByteArray()) {
                output.write(byte.toInt()); output.flush()
                if (trickle) Thread.sleep(10)
            }
        }

        override fun close() {
            server.close(); accepted.get()?.close(); worker.join(2000)
            assertFalse("T464: local fixture leaked its worker", worker.isAlive)
        }
    }

    @Test fun t464_httpAdapterReturnsNewerVersionFromLocalEndpoint() {
        Endpoint().use { endpoint ->
            val result = LinkedBlockingQueue<String>()
            HttpReleaseChecks(endpoint.url).newCall("1.0.0").start { result.add(it ?: "none") }
            assertEquals("9.0.0", result.poll(2, TimeUnit.SECONDS))
        }
    }

    @Test fun t464_wholeCallDeadlineBoundsTricklingBody() {
        Endpoint(trickle = true).use { endpoint ->
            val result = LinkedBlockingQueue<String>()
            val began = System.nanoTime()
            val call = HttpReleaseChecks(endpoint.url, deadlineMillis = 250).newCall("1.0.0")
            try {
                call.start { result.add(it ?: "none") }
                assertEquals("T464: trickling body outlived whole-call deadline", "none", result.poll(2, TimeUnit.SECONDS))
                assertTrue(TimeUnit.NANOSECONDS.toMillis(System.nanoTime() - began) < 1500)
            } finally { call.cancel() }
        }
    }

    @Test fun t464_cancelInterruptsActiveBodyRead() {
        Endpoint(trickle = true).use { endpoint ->
            val result = LinkedBlockingQueue<String>()
            val call = HttpReleaseChecks(endpoint.url).newCall("1.0.0")
            try {
                call.start { result.add(it ?: "none") }
                assertTrue(endpoint.headers.await(2, TimeUnit.SECONDS))
                call.cancel()
                assertEquals("none", result.poll(2, TimeUnit.SECONDS))
            } finally { call.cancel() }
        }
    }
}
