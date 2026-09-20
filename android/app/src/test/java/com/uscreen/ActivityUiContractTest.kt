@file:OptIn(androidx.compose.runtime.InternalComposeTracingApi::class)

package com.uscreen

import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.AndroidComposeTestRule
import okhttp3.*
import okio.ByteString
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.annotation.LooperMode
import org.robolectric.util.ReflectionHelpers

@RunWith(RobolectricTestRunner::class)
// The Activity installs Compose during onCreate, unlike rule.setContent tests.
// Keep its AndroidUiDispatcher in a separate instrumented Robolectric sandbox:
// reusing that dispatcher across reset loopers poisons later Compose tests.
// https://github.com/robolectric/robolectric/issues/7055
@Config(sdk = [27, 34], qualifiers = "w960dp-h600dp-land",
    instrumentedPackages = ["androidx.compose.ui.platform"])
@LooperMode(LooperMode.Mode.PAUSED)
class ActivityUiContractTest {
    class CompositionTrace : androidx.compose.runtime.CompositionTracer {
        val starts = java.util.concurrent.atomic.AtomicInteger()
        val ends = java.util.concurrent.atomic.AtomicInteger()
        override fun isTraceInProgress() = true
        override fun traceEventStart(key: Int, dirty1: Int, dirty2: Int, info: String) { starts.incrementAndGet() }
        override fun traceEventEnd() { ends.incrementAndGet() }
    }

    class ActivityFixture : org.junit.rules.ExternalResource() {
        lateinit var controller: org.robolectric.android.controller.ActivityController<MainActivity>
        lateinit var prefs: Prefs
        lateinit var capture: TouchCapture
        lateinit var receiver: VideoReceiver
        internal val socket = OfflineSocket()
        val trace = CompositionTrace()
        private var previousTracer: Any? = null
        private var stopped = false

        override fun before() {
            previousTracer = ReflectionHelpers.getStaticField(Class.forName("androidx.compose.runtime.ComposerKt"), "compositionTracer")
            androidx.compose.runtime.Composer.setTracer(trace)
            prefs = Prefs(RuntimeEnvironment.getApplication()).apply { checkUpdates = false }
            controller = Robolectric.buildActivity(MainActivity::class.java).create()
            val activity = controller.get()
            capture = TouchCapture(WebSocket.Factory { _, _ -> socket })
            receiver = VideoReceiver { error("T497 no physical video connection") }
            activity.session.stop()
            ReflectionHelpers.setField(activity, "session", SessionCoordinator(prefs, { it() }, receiver, capture))
            controller.start().resume().visible()
        }

        fun stop() {
            if (stopped) return
            stopped = true
            val content = controller.get().findViewById<android.view.ViewGroup>(android.R.id.content)
            (content.getChildAt(0) as? androidx.compose.ui.platform.AbstractComposeView)?.disposeComposition()
            controller.pause().stop().destroy()
            org.robolectric.Shadows.shadowOf(android.os.Looper.getMainLooper()).idle()
            ReflectionHelpers.setStaticField(Class.forName("androidx.compose.runtime.ComposerKt"), "compositionTracer", previousTracer)
        }

        override fun after() = stop()
    }

    private val fixture = ActivityFixture()
    @get:Rule val compose = AndroidComposeTestRule(fixture) { it.controller.get() }

    internal class OfflineSocket : WebSocket {
        var retired = false
        override fun request() = Request.Builder().url("ws://localhost/").build()
        override fun queueSize() = 0L
        override fun send(text: String) = true
        override fun send(bytes: ByteString) = true
        override fun close(code: Int, reason: String?): Boolean { retired = true; return true }
        override fun cancel() { retired = true }
    }

    @Test fun t497_realActivityCompositionRoutesDisplayChangesAndRetiresItsSession() {
        val activity = compose.activity
        val prefs = fixture.prefs
        try {
            compose.onNodeWithText("⚙").performClick()
            compose.onNodeWithContentDescription("Brightness").performScrollTo()
                .performSemanticsAction(SemanticsActions.SetProgress) { it(25f) }
            compose.runOnIdle {
                assertEquals(25, prefs.brightnessPercent)
                assertEquals(0.25f, activity.window.attributes.screenBrightness, 0.001f)
                assertFalse(fixture.capture.isControlConnected())
            }
            compose.onNodeWithText("System default").performScrollTo().performClick().assertIsSelected()
            compose.runOnIdle { assertEquals(0f, prefs.displayRefreshRate, 0f) }
            compose.onNodeWithText("Apply").performScrollTo().performClick()
            compose.onNodeWithText("Settings").assertDoesNotExist()
            compose.waitForIdle()
        } finally {
            fixture.stop()
        }
        assertTrue(fixture.socket.retired)
        assertNull(fixture.receiver.decoder.mediaCodec)
        assertTrue("T497 Activity composition never emitted its trace", fixture.trace.starts.get() > 0)
        assertEquals("T497 unbalanced Activity composition trace", fixture.trace.starts.get(), fixture.trace.ends.get())
    }

    @Test fun t539_cameraSelectionUsesAndroidPermissionResultAndForegroundOwner() {
        val activity = compose.activity
        org.robolectric.Shadows.shadowOf(RuntimeEnvironment.getApplication())
            .denyPermissions(android.Manifest.permission.CAMERA)
        try {
            compose.runOnIdle {
                CameraInvitations.endpoint.value = CameraEndpoint("a".repeat(64), 12345, 1280, 720, 30, 3000)
            }
            compose.onNodeWithText("⚙").performClick()
            compose.onNodeWithText("Front").performScrollTo().performClick()
            val permission = org.robolectric.Shadows.shadowOf(activity).nextStartedActivityForResult
            assertNotNull("T539 camera selection bypassed permission request", permission)
            assertNull(activity.cameras.selected)
            compose.runOnIdle {
                org.robolectric.Shadows.shadowOf(RuntimeEnvironment.getApplication())
                    .grantPermissions(android.Manifest.permission.CAMERA)
                activity.onRequestPermissionsResult(permission.requestCode,
                    arrayOf(android.Manifest.permission.CAMERA), intArrayOf(android.content.pm.PackageManager.PERMISSION_GRANTED))
            }
            compose.waitUntil(5000) {
                org.robolectric.Shadows.shadowOf(android.os.Looper.getMainLooper()).idle()
                activity.cameras.status == "Front camera unavailable"
            }
            assertNull(activity.cameras.selected)
        } finally {
            CameraInvitations.endpoint.value = null
            fixture.stop()
        }
        assertNull(activity.cameras.endpoint)
    }
}
