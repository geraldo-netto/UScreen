package com.uscreen

import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.pm.ApplicationInfo
import android.view.MotionEvent
import android.view.OrientationEventListener
import okhttp3.*
import okio.ByteString
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34])
class RegressionTest {
    private val app get() = RuntimeEnvironment.getApplication()
    private fun owner(target: Any, name: String): Any = when (target) {
        is MainActivity -> if (name == "tiltListener") target.windowPolicy else target.session
        is VideoReceiver -> videoOwner(target, name)
        is TouchCapture -> if (name == "touchSlots") target.motion else target.control
        else -> target
    }
    private fun videoOwner(target: VideoReceiver, name: String): Any = when (name) {
        "mediaCodec" -> target.decoder
        "socket" -> target.transport
        else -> target
    }
    private fun get(target: Any, name: String): Any? {
        val owner = owner(target, name)
        return owner.javaClass.getDeclaredField(name).apply { isAccessible = true }.get(owner)
    }
    private fun set(target: Any, name: String, value: Any?) {
        val owner = owner(target, name)
        owner.javaClass.getDeclaredField(name).apply { isAccessible = true }.set(owner, value)
    }

    @Test fun t312_trafficRatesUseMonotonicElapsedTime() {
        val statistics = ReceiverStatistics()
        var now = System.nanoTime()
        statistics.sample(now)
        for (duration in listOf(2_000_000_000L, 500_000_000L, 1_250_000_000L)) {
            repeat(30) { statistics.frameRendered() }
            statistics.bytesReceived(1_000_000)
            now += duration
            statistics.sample(now)
            val seconds = duration / 1_000_000_000.0
            assertEquals("T312 delayed FPS", (30 / seconds).toFloat(), statistics.fps, 0.0001f)
            assertEquals("T312 delayed Mbps", (8 / seconds).toFloat(), statistics.mbps, 0.0001f)
        }
        statistics.sample(now + 1_000_000_000L)
        assertEquals(0f, statistics.fps, 0f)
        assertEquals(0f, statistics.mbps, 0f)
    }

    @Test fun t312_nonpositiveIntervalsKeepPendingCounts() {
        val statistics = ReceiverStatistics()
        val now = System.nanoTime()
        statistics.sample(now)
        statistics.frameRendered()
        statistics.bytesReceived(125_000)
        for (invalid in listOf(now, now - 1)) {
            statistics.sample(invalid)
            assertEquals(0f, statistics.fps, 0f)
            assertEquals(0f, statistics.mbps, 0f)
        }
        statistics.sample(now + 1_000_000_000L)
        assertEquals(1f, statistics.fps, 0f)
        assertEquals(1f, statistics.mbps, 0f)
    }

    @Test fun t309_receiverRestartsRetireTrafficStatistics() {
        val receiver = VideoReceiver { error("T309 must not open a real socket") }
        receiver.start() // No surface: network and decoder workers remain idle.
        val retired = get(receiver, "statistics") as ReceiverStatistics
        val retiredAt = System.nanoTime()
        retired.sample(retiredAt)
        try {
            repeat(30) { retired.frameRendered() }
            retired.bytesReceived(1_000_000)
            retired.sample(retiredAt + 1_000_000_000L)
            assertEquals(30f, receiver.getFps(), 0f)
            assertEquals(8f, receiver.getMbps(), 0f)
            repeat(7) { retired.frameRendered() }
            retired.bytesReceived(500_000) // Pending sample at the moment of stop.
            receiver.stop()
            assertEquals("T309 stopped receiver retained FPS", 0f, receiver.getFps(), 0f)
            assertEquals("T309 stopped receiver retained bitrate", 0f, receiver.getMbps(), 0f)
            receiver.start()
            assertEquals(0f, receiver.getFps(), 0f)
            assertEquals(0f, receiver.getMbps(), 0f)
            val current = get(receiver, "statistics") as ReceiverStatistics
            val currentAt = System.nanoTime()
            current.sample(currentAt)
            current.frameRendered()
            current.bytesReceived(125_000)
            current.sample(currentAt + 1_000_000_000L)
            assertEquals("T309 previous run contaminated new FPS", 1f, receiver.getFps(), 0f)
            assertEquals("T309 previous run contaminated new bitrate", 1f, receiver.getMbps(), 0f)
            retired.frameRendered()
            retired.bytesReceived(1_000_000)
            retired.sample(retiredAt + 2_000_000_000L) // A delayed worker from the retired generation.
            assertEquals(1f, receiver.getFps(), 0f)
            assertEquals(1f, receiver.getMbps(), 0f)
        } finally { receiver.stop() }
    }

    @Test fun t319_latePalmClassificationReleasesOnlyItsExistingTouch() {
        for (palmAction in listOf(MotionEvent.ACTION_MOVE, MotionEvent.ACTION_POINTER_UP)) {
            val capture = TouchCapture()
            val socket = Socket()
            set(capture, "webSocket", socket)
            set(capture, "isConnected", true)
            fun send(action: Int, vararg pointers: Pair<Int, Int>): List<org.json.JSONObject> {
                socket.messages.clear()
                val motion = MotionEvent.obtain(0, 10, action, pointers.size,
                    pointers.map { (pointerId, tool) -> MotionEvent.PointerProperties().apply {
                        id = pointerId; toolType = tool
                    } }.toTypedArray(),
                    pointers.map { MotionEvent.PointerCoords().apply {
                        x = 30f; y = 40f; pressure = 0.5f
                    } }.toTypedArray(), 0, 0, 1f, 1f, 0, 0, 0, 0)
                try { capture.handleMotionEvent(motion, 100, 100) }
                finally { motion.recycle() }
                return socket.messages.map { org.json.JSONObject(it) }
            }
            val finger = MotionEvent.TOOL_TYPE_FINGER
            try {
                val first = send(MotionEvent.ACTION_DOWN, 10 to finger).single().getInt("slot")
                val second = send(MotionEvent.ACTION_POINTER_DOWN or (1 shl 8),
                    10 to finger, 11 to finger).single().getInt("slot")
                val rejected = send(palmAction, 10 to 5, 11 to finger)
                val releases = rejected.filter { it.getInt("action") == 1 }
                assertEquals("T319 palm action $palmAction left the old contact down",
                    listOf(first), releases.map { it.getInt("slot") })
                assertEquals(0.0, releases.single().getDouble("pressure"), 0.0)
                assertEquals(mapOf(11 to second), get(capture, "touchSlots"))
                val continued = send(MotionEvent.ACTION_MOVE, 11 to finger).single()
                assertEquals(2, continued.getInt("action"))
                assertEquals(second, continued.getInt("slot"))
                val replacement = send(MotionEvent.ACTION_POINTER_DOWN or (1 shl 8),
                    11 to finger, 31 to finger).single()
                assertEquals(first, replacement.getInt("slot"))
                assertEquals(setOf(first, second),
                    send(MotionEvent.ACTION_CANCEL, 11 to finger, 31 to finger)
                        .map { it.getInt("slot") }.toSet())
            } finally { capture.disconnect() }
        }
    }

    @Test fun t303_platformPalmCannotBecomeAFingerContact() {
        // AOSP's hidden MotionEvent.TOOL_TYPE_PALM is 5, not an SDK API.
        val capture = TouchCapture()
        val socket = Socket()
        set(capture, "webSocket", socket)
        set(capture, "isConnected", true)
        try {
            for (action in listOf(MotionEvent.ACTION_DOWN, MotionEvent.ACTION_MOVE, MotionEvent.ACTION_UP)) {
                val motion = event(5, action)
                try { capture.handleMotionEvent(motion, 100, 100) }
                finally { motion.recycle() }
            }
            assertTrue("T303: palm produced host input: ${socket.messages}", socket.messages.isEmpty())
            for (tool in listOf(MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS,
                                MotionEvent.TOOL_TYPE_ERASER, MotionEvent.TOOL_TYPE_MOUSE)) {
                socket.messages.clear()
                for (action in listOf(MotionEvent.ACTION_DOWN, MotionEvent.ACTION_UP)) {
                    val motion = event(tool, action)
                    try { capture.handleMotionEvent(motion, 100, 100) }
                    finally { motion.recycle() }
                }
                val messages = socket.messages.map { org.json.JSONObject(it) }
                assertEquals("T303: standard tool $tool lost contact events", listOf(0, 1),
                    messages.map { it.getInt("action") })
                val type = if (tool in listOf(MotionEvent.TOOL_TYPE_STYLUS, MotionEvent.TOOL_TYPE_ERASER)) "pen" else "touch"
                assertEquals(listOf(type, type), messages.map { it.getString("type") })
            }
        } finally { capture.disconnect() }
    }

    @Test fun t282_concurrentControlMessagesFollowAuthentication() {
        assertAuthFirst("config") { it.sendConfig(2000, 30) }
        assertAuthFirst("mode") { it.sendMode(true) }
        assertAuthFirst("rendered") { it.sendRendered(12, 100) }
        for (action in listOf(MotionEvent.ACTION_HOVER_MOVE, MotionEvent.ACTION_HOVER_EXIT,
                              MotionEvent.ACTION_BUTTON_PRESS, MotionEvent.ACTION_BUTTON_RELEASE)) {
            assertAuthFirst("pen") { capture ->
                val motion = event(MotionEvent.TOOL_TYPE_STYLUS, action)
                try { capture.handleHoverEvent(motion, 100, 100) }
                finally { motion.recycle() }
            }
        }
    }

    private fun controlWorker(failures: java.util.Queue<Throwable>, action: () -> Unit): Thread =
        Thread { try { action() } catch (failure: Throwable) { failures.add(failure) } }.apply { start() }

    private fun awaitBlockedOrFinished(thread: Thread) {
        val deadline = System.nanoTime() + java.util.concurrent.TimeUnit.SECONDS.toNanos(3)
        while (thread.isAlive && thread.state != Thread.State.BLOCKED && System.nanoTime() < deadline) {
            Thread.sleep(1)
        }
        assertTrue("T282 worker must reach the send or connection monitor",
            !thread.isAlive || thread.state == Thread.State.BLOCKED)
    }

    private fun assertAuthFirst(type: String, action: (TouchCapture) -> Unit) {
        val capture = TouchCapture().apply { token = "a".repeat(64) }
        val authEntered = java.util.concurrent.CountDownLatch(1)
        val authRelease = java.util.concurrent.CountDownLatch(1)
        val socket = Socket {
            if (org.json.JSONObject(it).getString("type") == "auth") {
                authEntered.countDown()
                check(authRelease.await(5, java.util.concurrent.TimeUnit.SECONDS))
            }
        }
        set(capture, "webSocket", socket)
        val listener = get(capture, "wsListener") as WebSocketListener
        val response = Response.Builder().request(socket.request()).protocol(Protocol.HTTP_1_1)
            .code(101).message("Switching Protocols").build()
        val failures = java.util.concurrent.ConcurrentLinkedQueue<Throwable>()
        val opener = controlWorker(failures) { listener.onOpen(socket, response) }
        var sender: Thread? = null
        try {
            assertTrue(authEntered.await(3, java.util.concurrent.TimeUnit.SECONDS))
            sender = controlWorker(failures) { action(capture) }
            awaitBlockedOrFinished(sender)
        } finally {
            authRelease.countDown()
            opener.join(3000)
            sender?.join(3000)
            capture.disconnect()
        }
        assertFalse(opener.isAlive)
        assertFalse(sender!!.isAlive)
        assertTrue("T282 worker failed: $failures", failures.isEmpty())
        val types = socket.messages.map { org.json.JSONObject(it).getString("type") }
        assertEquals("T282 $type overtook authentication: $types", "auth", types.first())
        assertTrue("T282 lost requested $type: $types", types.drop(1).contains(type))
    }

    @Test fun t253_arrivalHistorySurvivesCounterOverflowAndRingWraps() {
        val timing = FrameTiming()
        set(timing, "arrivalWrite", Int.MAX_VALUE - 1)
        val first = 1000
        val last = first + VideoReceiver.ARRIVAL_RING * 2
        for (seq in first..last) {
            timing.noteArrival(seq)
            timing.noteReleased(seq)
            assertTrue("T253: just-arrived frame must retain its timestamp", timing.decodeMicrosFor(seq) >= 0)
        }
        assertEquals(-1, timing.decodeMicrosFor(first))
        assertTrue(timing.decodeMicrosFor(last - VideoReceiver.ARRIVAL_RING + 1) >= 0)
    }

    @Test fun t133_tokenRotationRestartsActiveReconnectsOnly() {
        val prefs = Prefs(app).apply { checkUpdates = false; hostToken = "a".repeat(64) }
        val controller = Robolectric.buildActivity(MainActivity::class.java).create()
        val activity = controller.get()
        val capture = get(activity, "touchCapture") as TouchCapture
        val receiver = get(activity, "videoReceiver") as VideoReceiver
        try {
            set(activity, "started", true)
            receiver.start() // No surface; session waits without real video I/O.
            assertFalse(capture.isControlConnected())
            val generation = capture.connectionGeneration
            val oldJob = get(receiver, "job") as kotlinx.coroutines.Job
            prefs.hostToken = "b".repeat(64)
            activity.session.applyToken(true)
            assertTrue("Disconnected control session must rotate", capture.connectionGeneration > generation)
            assertTrue("Video retry must retire captured old token", oldJob.isCancelled)
            assertNotSame(oldJob, get(receiver, "job"))
            assertEquals(prefs.hostToken, receiver.token)
            assertEquals(prefs.hostToken, capture.token)

            set(activity, "started", false)
            capture.disconnect()
            receiver.stop()
            val stoppedGeneration = capture.connectionGeneration
            val stoppedJob = get(receiver, "job")
            prefs.hostToken = "c".repeat(64)
            activity.session.applyToken(true)
            assertEquals(stoppedGeneration, capture.connectionGeneration)
            assertSame(stoppedJob, get(receiver, "job"))
            assertFalse(get(receiver, "isRunning") as Boolean)
        } finally {
            set(activity, "started", false)
            capture.disconnect()
            receiver.stop()
            controller.destroy()
        }
    }

    @Test fun t123_updateVersionsFollowSharedValidationAndPrecedence() {
        val fixture = javaClass.classLoader!!.getResourceAsStream("version-comparisons.tsv")!!
        fixture.bufferedReader().useLines { lines -> lines.forEach { line ->
            val parts = line.split('\t')
            assertEquals(line, parts[2] == "true", UpdateCheck.isNewer(parts[0], parts[1]))
        } }
    }

    @Test fun t036_sessionTokenIsExcludedFromBackup() {
        assertEquals(0, app.applicationInfo.flags and ApplicationInfo.FLAG_ALLOW_BACKUP)
    }

    @Test fun t037_cleartextPolicyOnlyPermitsLoopback() {
        val id = app.resources.getIdentifier("network_security_config", "xml", app.packageName)
        assertTrue("Scoped network security policy must exist", id != 0)
        val parser = app.resources.getXml(id)
        var baseDisallows = false
        val domains = mutableListOf<String>()
        while (parser.next() != org.xmlpull.v1.XmlPullParser.END_DOCUMENT) {
            if (parser.eventType != org.xmlpull.v1.XmlPullParser.START_TAG) continue
            when (parser.name) {
                "base-config" -> baseDisallows = parser.getAttributeValue(null, "cleartextTrafficPermitted") == "false"
                "domain-config" -> assertEquals("true", parser.getAttributeValue(null, "cleartextTrafficPermitted"))
                "domain" -> domains.add(parser.nextText())
            }
        }
        assertTrue(baseDisallows)
        assertEquals(listOf("127.0.0.1"), domains)
    }

    @Test fun t250_forkPackageKeepsProtectedOriginalClasses() {
        assertEquals("io.github.geraldo_netto.uscreen", app.packageName)
        for (name in listOf("MainActivity", "TokenActivity")) {
            val component = ComponentName(app.packageName, "com.uscreen.$name")
            val info = app.packageManager.getActivityInfo(component, 0)
            assertEquals("com.uscreen.$name", info.name)
        }
        val receiver = ComponentName(app.packageName, "com.uscreen.TokenReceiver")
        assertEquals("android.permission.DUMP", app.packageManager.getReceiverInfo(receiver, 0).permission)
    }

    @Test fun t039_launcherIntentCannotOverwriteTrustedToken() {
        val prefs = Prefs(app).apply { hostToken = "trusted"; checkUpdates = false }
        val controller = Robolectric.buildActivity(MainActivity::class.java, Intent().putExtra("token", "untrusted")).create()
        try {
            assertEquals("trusted", prefs.hostToken)
            controller.newIntent(Intent().putExtra("token", "untrusted-again"))
            assertEquals("trusted", prefs.hostToken)
        } finally { controller.destroy() }
    }

    @Test fun t039_tokenDeliveryRequiresShellPermissionAndFrontsLauncher() {
        val component = ComponentName(app.packageName, "com.uscreen.TokenActivity")
        val info = app.packageManager.getActivityInfo(component, 0)
        assertEquals("android.permission.DUMP", info.permission)
        assertTrue(info.exported)
        val token = "a".repeat(64)
        val controller = Robolectric.buildActivity(TokenActivity::class.java,
            Intent().setComponent(component).putExtra("token", token)).create()
        try {
            assertEquals(token, Prefs(app).hostToken)
            val next = org.robolectric.Shadows.shadowOf(controller.get()).nextStartedActivity
            assertEquals(MainActivity::class.java.name, next.component?.className)
            assertFalse(next.hasExtra("token"))
        } finally { controller.destroy() }
    }

    @Test fun t040_backgroundActivityDisablesTiltSensor() {
        Prefs(app).checkUpdates = false
        val controller = Robolectric.buildActivity(MainActivity::class.java).create()
        val activity = controller.get()
        // Avoid network work; exercise the real activity lifecycle with a recording sensor.
        set(activity, "touchCapture", null)
        set(activity, "videoReceiver", null)
        controller.start()
        var disabled = false
        set(activity, "tiltListener", object : OrientationEventListener(activity) {
            override fun onOrientationChanged(orientation: Int) {}
            override fun disable() { disabled = true }
        })
        controller.stop()
        try { assertTrue("onStop must unregister the sensor", disabled) }
        finally { controller.destroy() }
    }

    @Test fun t041_oneMbpsSettingMatchesHostMinimum() {
        val prefs = Prefs(app)
        prefs.bitrateKbps = 1000
        assertEquals(1000, prefs.bitrateKbps)
    }

    @Test fun t042_controlSocketPingsDetectDeadLinks() {
        val capture = TouchCapture()
        assertEquals(5000, (get(capture, "client") as OkHttpClient).pingIntervalMillis)
    }

    @Test fun t112_replacedAndDisconnectedCallbacksCannotResurrectControl() {
        val capture = TouchCapture()
        val old = Socket()
        val current = Socket()
        val listener = get(capture, "wsListener") as WebSocketListener
        var callbacks = 0
        capture.token = "a".repeat(64)
        capture.sendConfig(2000, 30)
        capture.onModeKnown = { callbacks++ }
        capture.onCodecKnown = { callbacks++ }
        set(capture, "webSocket", current)
        val response = Response.Builder().request(old.request()).protocol(Protocol.HTTP_1_1).code(101).message("Switching Protocols").build()
        listener.onOpen(old, response)
        listener.onMessage(old, """{"pen_only":true,"codec":"hevc"}""")
        assertFalse(capture.isControlConnected())
        assertEquals(0, callbacks)
        assertTrue(old.messages.isEmpty())
        listener.onOpen(current, response)
        assertTrue(capture.isControlConnected())
        assertTrue(current.messages.isNotEmpty())
        capture.disconnect()
        current.messages.clear()
        listener.onOpen(current, response)
        listener.onMessage(current, """{"pen_only":false,"codec":"h264"}""")
        assertFalse(capture.isControlConnected())
        assertEquals(0, callbacks)
        assertTrue(current.messages.isEmpty())
    }

    @Test fun t112_closedSocketCannotOpenAgain() {
        for (failed in listOf(false, true)) {
            val capture = TouchCapture()
            val socket = Socket()
            val listener = get(capture, "wsListener") as WebSocketListener
            val response = Response.Builder().request(socket.request()).protocol(Protocol.HTTP_1_1).code(101).message("Switching Protocols").build()
            set(capture, "webSocket", socket)
            listener.onOpen(socket, response)
            if (failed) listener.onFailure(socket, java.io.IOException("closed"), null)
            else listener.onClosed(socket, 1000, "closed")
            listener.onOpen(socket, response)
            try { assertFalse(capture.isControlConnected()) }
            finally { capture.disconnect() }
        }
    }

    @Test fun t112_backgroundModeAndCodecCallbacksCannotStartVideo() {
        Prefs(app).checkUpdates = false
        val controller = Robolectric.buildActivity(MainActivity::class.java).create()
        val activity = controller.get()
        val capture = get(activity, "touchCapture") as TouchCapture
        val receiver = get(activity, "videoReceiver") as VideoReceiver
        try {
            assertFalse(get(activity, "started") as Boolean)
            capture.onModeKnown?.invoke(false)
            capture.onCodecKnown?.invoke("hevc")
            org.robolectric.Shadows.shadowOf(android.os.Looper.getMainLooper()).idle()
            assertFalse("Background callback started video", get(receiver, "isRunning") as Boolean)
        } finally { controller.destroy() }
    }

    @Test @org.robolectric.annotation.LooperMode(org.robolectric.annotation.LooperMode.Mode.PAUSED)
    fun t112_queuedGreetingCannotAffectReplacementSession() {
        Prefs(app).checkUpdates = false
        val controller = Robolectric.buildActivity(MainActivity::class.java).create()
        val activity = controller.get()
        val capture = get(activity, "touchCapture") as TouchCapture
        val receiver = get(activity, "videoReceiver") as VideoReceiver
        try {
            set(activity, "started", true)
            val worker = Thread { capture.onModeKnown?.invoke(false); capture.onCodecKnown?.invoke("hevc") }
            worker.start()
            worker.join()
            capture.disconnect()
            org.robolectric.Shadows.shadowOf(android.os.Looper.getMainLooper()).idle()
            assertFalse("Queued old greeting started video", get(receiver, "isRunning") as Boolean)
            assertEquals(VideoReceiver.MIME_TYPE, receiver.mimeType)
        } finally { set(activity, "started", false); controller.destroy() }
    }

    @Test fun t146_motionCoordinatesStayNormalizedIncludingReleases() {
        for (tool in listOf(MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)) {
            val capture = TouchCapture()
            val socket = Socket()
            set(capture, "webSocket", socket)
            set(capture, "isConnected", true)
            fun send(action: Int, x: Float, y: Float): org.json.JSONObject {
                socket.messages.clear()
                val e = MotionEvent.obtain(0, 10, action, 1,
                    arrayOf(MotionEvent.PointerProperties().apply { id = 0; toolType = tool }),
                    arrayOf(MotionEvent.PointerCoords().apply { this.x = x; this.y = y; pressure = 2f }),
                    0, 0, 1f, 1f, 0, 0, 0, 0)
                try { assertTrue(capture.handleMotionEvent(e, 100, 100)) } finally { e.recycle() }
                return org.json.JSONObject(socket.messages.single())
            }
            val down = send(MotionEvent.ACTION_DOWN, -20f, 140f)
            assertEquals(0.0, down.getDouble("x"), 0.0)
            assertEquals(1.0, down.getDouble("y"), 0.0)
            assertEquals(1.0, down.getDouble("pressure"), 0.0)
            val move = send(MotionEvent.ACTION_MOVE, 100f, 0f)
            assertEquals(1.0, move.getDouble("x"), 0.0)
            assertEquals(0.0, move.getDouble("y"), 0.0)
            val up = send(MotionEvent.ACTION_UP, 150f, -50f)
            assertEquals(1.0, up.getDouble("x"), 0.0)
            assertEquals(0.0, up.getDouble("y"), 0.0)
            assertEquals(1, up.getInt("action"))
        }
    }

    @Test fun t087_sparsePointerIdsKeepDistinctSlotsAcrossReordering() {
        val capture = TouchCapture()
        val socket = Socket()
        set(capture, "webSocket", socket)
        set(capture, "isConnected", true)
        fun send(action: Int, vararg ids: Int): List<org.json.JSONObject> {
            socket.messages.clear()
            val e = MotionEvent.obtain(0, 10, action, ids.size,
                ids.map { MotionEvent.PointerProperties().apply { id = it; toolType = MotionEvent.TOOL_TYPE_FINGER } }.toTypedArray(),
                ids.map { MotionEvent.PointerCoords().apply { x = it.toFloat(); y = 40f; pressure = 0.5f } }.toTypedArray(),
                0, 0, 1f, 1f, 0, 0, 0, 0)
            try { capture.handleMotionEvent(e, 100, 100) } finally { e.recycle() }
            return socket.messages.map { org.json.JSONObject(it) }
        }
        val first = send(MotionEvent.ACTION_DOWN, 10).single().getInt("slot")
        val second = send(MotionEvent.ACTION_POINTER_DOWN or (1 shl 8), 10, 11).single().getInt("slot")
        assertNotEquals(first, second)
        assertEquals(listOf(second, first), send(MotionEvent.ACTION_MOVE, 11, 10).map { it.getInt("slot") })
        assertEquals(first, send(MotionEvent.ACTION_POINTER_UP or (1 shl 8), 11, 10).single().getInt("slot"))
        assertEquals(first, send(MotionEvent.ACTION_POINTER_DOWN or (1 shl 8), 11, 31).single().getInt("slot"))
        assertEquals(setOf(first, second), send(MotionEvent.ACTION_CANCEL, 11, 31).map { it.getInt("slot") }.toSet())
        assertEquals(first, send(MotionEvent.ACTION_DOWN, 7).single().getInt("slot"))
        send(MotionEvent.ACTION_CANCEL, 7)
        val allocated = mutableSetOf<Int>()
        for (i in 0..10) {
            val packets = send(if (i == 0) MotionEvent.ACTION_DOWN else MotionEvent.ACTION_POINTER_DOWN or (i shl 8), *(10..(10 + i)).toList().toIntArray())
            if (i < 10) assertTrue(allocated.add(packets.single().getInt("slot")))
            else assertTrue("Eleventh contact must not alias an active slot", packets.isEmpty())
        }
        assertEquals((0..9).toSet(), allocated)
        assertEquals(10, send(MotionEvent.ACTION_CANCEL, *(10..20).toList().toIntArray()).size)
    }

    private class VideoSocket : java.net.Socket() {
        val reading = java.util.concurrent.CountDownLatch(1)
        val releaseRead = java.util.concurrent.CountDownLatch(1)
        @Volatile var closed = false
        override fun connect(endpoint: java.net.SocketAddress?, timeout: Int) {}
        override fun setTcpNoDelay(value: Boolean) {}
        override fun setSoTimeout(value: Int) {}
        override fun setReceiveBufferSize(value: Int) {}
        override fun getInputStream() = object : java.io.InputStream() {
            override fun read(): Int = error("use bulk read")
            override fun read(buffer: ByteArray, offset: Int, length: Int): Int {
                reading.countDown()
                check(releaseRead.await(3, java.util.concurrent.TimeUnit.SECONDS))
                throw java.io.EOFException()
            }
        }
        override fun getOutputStream() = java.io.ByteArrayOutputStream()
        override fun close() { closed = true }
    }

    @Test fun t088_lateOldCleanupLeavesReplacementSocketAndDecoderAlive() {
        val old = VideoSocket()
        val next = VideoSocket()
        val sockets = java.util.concurrent.LinkedBlockingQueue<java.net.Socket>().apply { add(old); add(next) }
        val receiver = VideoReceiver { sockets.remove() }
        (get(receiver, "surfaceReady") as java.util.concurrent.atomic.AtomicBoolean).set(true)
        set(receiver, "mediaCodec", android.media.MediaCodec.createDecoderByType(VideoReceiver.MIME_TYPE))
        try {
            receiver.start()
            assertTrue(old.reading.await(2, java.util.concurrent.TimeUnit.SECONDS))
            val oldJob = get(receiver, "job") as kotlinx.coroutines.Job
            receiver.stop()
            val nextCodec = android.media.MediaCodec.createDecoderByType(VideoReceiver.MIME_TYPE)
            set(receiver, "mediaCodec", nextCodec)
            receiver.start()
            assertTrue(next.reading.await(2, java.util.concurrent.TimeUnit.SECONDS))
            old.releaseRead.countDown()
            kotlinx.coroutines.runBlocking { kotlinx.coroutines.withTimeout(2000) { oldJob.join() } }
            assertFalse("Old cleanup closed the replacement socket", next.closed)
            assertSame(next, get(receiver, "socket"))
            assertSame(nextCodec, get(receiver, "mediaCodec"))
            assertTrue(get(receiver, "isRunning") as Boolean)
        } finally {
            receiver.stop()
            old.releaseRead.countDown()
            next.releaseRead.countDown()
        }
    }

    @Test fun t120_hostGreetingUpdatesEffectiveDecoderRate() {
        Prefs(app).checkUpdates = false
        val controller = Robolectric.buildActivity(MainActivity::class.java).create()
        val activity = controller.get()
        val capture = get(activity, "touchCapture") as TouchCapture
        val receiver = get(activity, "videoReceiver") as VideoReceiver
        val socket = Socket()
        set(capture, "webSocket", socket)
        set(activity, "started", true)
        val listener = get(capture, "wsListener") as WebSocketListener
        try {
            listener.onMessage(socket, """{"fps":30}""")
            org.robolectric.Shadows.shadowOf(android.os.Looper.getMainLooper()).idle()
            assertEquals(30, receiver.streamFps)
            listener.onMessage(socket, """{"fps":90}""")
            org.robolectric.Shadows.shadowOf(android.os.Looper.getMainLooper()).idle()
            assertEquals(90, receiver.streamFps)
            listener.onMessage(socket, """{"fps":999}""")
            assertEquals(90, receiver.streamFps)
        } finally { set(activity, "started", false); controller.destroy() }
    }

    @Test fun t120_applyRebuildsRunningDecoderWithSelectedRate() {
        val receiver = VideoReceiver()
        val prefs = Prefs(app)
        receiver.start()
        val previous = get(receiver, "job")
        try {
            applyStreamSettings(prefs, null, receiver, 5000, 30)
            assertEquals(30, receiver.streamFps)
            assertEquals(30, prefs.fps)
            assertNotSame("New rate needs a fresh decoder session", previous, get(receiver, "job"))
            assertTrue(get(receiver, "isRunning") as Boolean)
        } finally { receiver.stop() }
    }

    private class Socket(private val beforeSend: (String) -> Unit = {}) : WebSocket {
        val messages = java.util.concurrent.CopyOnWriteArrayList<String>()
        override fun request() = Request.Builder().url(TouchCapture.WS_URL).build()
        override fun queueSize() = 0L
        override fun send(text: String): Boolean { beforeSend(text); messages.add(text); return true }
        override fun send(bytes: ByteString) = false
        override fun close(code: Int, reason: String?) = true
        override fun cancel() {}
    }

    private fun event(tool: Int, action: Int): MotionEvent = MotionEvent.obtain(
        0, 10, action, 1,
        arrayOf(MotionEvent.PointerProperties().apply { id = 0; toolType = tool }),
        arrayOf(MotionEvent.PointerCoords().apply { x = 30f; y = 40f; pressure = 0.5f }),
        0, 0, 1f, 1f, 0, 0, 0, 0
    )

    @Test fun t062_disabledDevicesSendNoTouchPenHoverOrButtonEvents() {
        val capture = TouchCapture()
        val socket = Socket()
        set(capture, "webSocket", socket)
        set(capture, "isConnected", true)
        val listener = get(capture, "wsListener") as WebSocketListener
        listener.onMessage(socket, """{"touch":false,"pen":false,"pen_only":false}""")
        for (tool in listOf(MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)) {
            for (action in listOf(MotionEvent.ACTION_DOWN, MotionEvent.ACTION_MOVE, MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL, MotionEvent.ACTION_HOVER_MOVE, MotionEvent.ACTION_HOVER_EXIT, MotionEvent.ACTION_BUTTON_PRESS)) {
                val e = event(tool, action)
                capture.handleMotionEvent(e, 100, 100)
                capture.handleHoverEvent(e, 100, 100)
                e.recycle()
            }
        }
        assertTrue("Disabled input must not generate packets: ${socket.messages}", socket.messages.isEmpty())
        listener.onMessage(socket, """{"touch":true,"pen":true}""")
        val e = event(MotionEvent.TOOL_TYPE_FINGER, MotionEvent.ACTION_DOWN)
        capture.handleMotionEvent(e, 100, 100)
        e.recycle()
        assertEquals(1, socket.messages.size)
    }
}
