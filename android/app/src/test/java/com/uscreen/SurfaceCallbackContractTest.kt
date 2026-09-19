package com.uscreen

import android.view.SurfaceView
import android.view.View
import android.view.ViewGroup
import androidx.activity.ComponentActivity
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.LooperMode
import org.robolectric.shadows.ShadowSurfaceView.FakeSurfaceHolder

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34])
@LooperMode(LooperMode.Mode.PAUSED)
class SurfaceCallbackContractTest {
    @get:Rule val compose = createAndroidComposeRule<ComponentActivity>()

    @Test fun t497_surfaceRetirementAllowsAnAbsentObserver() {
        var ready: SurfaceView? = null
        compose.setContent { UScreenTheme { UScreenMain(onSurfaceReady = { ready = it }) } }
        compose.runOnIdle {
            val view = surface(compose.activity.findViewById(android.R.id.content))!!
            val holder = view.holder as FakeSurfaceHolder
            val callback = holder.callbacks.single()
            callback.surfaceCreated(holder)
            assertSame(view, ready)
            callback.surfaceDestroyed(holder)
            assertEquals(1, holder.callbacks.size)
        }
    }

    private fun surface(view: View): SurfaceView? {
        if (view is SurfaceView) return view
        if (view !is ViewGroup) return null
        for (index in 0 until view.childCount) surface(view.getChildAt(index))?.let { return it }
        return null
    }

    @Test fun t497_surfaceCallbacksPublishCurrentViewAndRetireOnDestruction() {
        val ready = mutableListOf<SurfaceView>()
        var destroyed = 0
        compose.setContent { UScreenTheme {
            UScreenMain(onSurfaceReady = { ready.add(it) }, onSurfaceDestroyed = { destroyed++ })
        } }
        compose.runOnIdle {
            val view = surface(compose.activity.findViewById(android.R.id.content))!!
            val holder = view.holder as FakeSurfaceHolder
            val callbacks = holder.callbacks.toList()
            assertEquals(1, callbacks.size)
            ready.clear()
            callbacks.single().surfaceCreated(holder)
            callbacks.single().surfaceChanged(holder, android.graphics.PixelFormat.OPAQUE, 0, Int.MAX_VALUE)
            assertEquals(listOf(view, view), ready)
            val before = destroyed
            callbacks.single().surfaceDestroyed(holder)
            assertEquals(before + 1, destroyed)
        }
    }
}
