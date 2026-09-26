package com.blent.benchmark

import android.app.Activity
import android.graphics.BitmapFactory
import android.os.Bundle
import android.view.View
import android.view.WindowManager
import android.widget.ImageView
import kotlin.math.abs

/** T388: static app-off control, with matching pixels, brightness and refresh. */
class MainActivity : Activity() {
    override fun onCreate(state: Bundle?) {
        super.onCreate(state)
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        window.attributes = window.attributes.apply {
            screenBrightness = 0.5f
            preferredRefreshRate = 60f
            @Suppress("DEPRECATION")
            val display = windowManager.defaultDisplay
            val current = display.mode
            preferredDisplayModeId = display.supportedModes.filter {
                it.physicalWidth == current.physicalWidth && it.physicalHeight == current.physicalHeight
            }.minByOrNull { abs(it.refreshRate - 60f) }?.modeId ?: current.modeId
        }
        immersive()
        val bitmap = assets.open("static.png").use { BitmapFactory.decodeStream(it) }
        check(bitmap.width == 1280 && bitmap.height == 800)
        setContentView(ImageView(this).apply { setImageBitmap(bitmap); scaleType = ImageView.ScaleType.FIT_CENTER })
    }
    override fun onWindowFocusChanged(focused: Boolean) {
        super.onWindowFocusChanged(focused)
        if (focused) immersive()
    }
    @Suppress("DEPRECATION")
    private fun immersive() {
        window.decorView.systemUiVisibility = View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY or
            View.SYSTEM_UI_FLAG_FULLSCREEN or View.SYSTEM_UI_FLAG_HIDE_NAVIGATION or
            View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN or View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION or
            View.SYSTEM_UI_FLAG_LAYOUT_STABLE
    }
}
