package com.uscreen.benchmark

import android.app.Activity
import android.os.Bundle
import android.view.SurfaceHolder
import android.view.SurfaceView
import android.view.WindowManager
import java.io.File
import java.util.concurrent.atomic.AtomicBoolean
import org.json.JSONObject

/** Shell-only, separate application. Closing/backgrounding the activity ends
 * its test; the experiment never updates installed UScreen preferences. */
class MainActivity : Activity(), SurfaceHolder.Callback {
    private val active = AtomicBoolean(false)
    private var worker: Thread? = null
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        window.attributes = window.attributes.apply { screenBrightness = 0.5f; preferredRefreshRate = 60f }
        setContentView(SurfaceView(this).apply { holder.addCallback(this@MainActivity) })
    }
    override fun surfaceCreated(holder: SurfaceHolder) {
        if (!intent.getBooleanExtra("run", false) || !active.compareAndSet(false, true)) return
        worker = Thread({ execute(holder) }, "decoder-bench-producer").apply { start() }
    }
    private fun execute(holder: SurfaceHolder) {
        val result = try {
            if (intent.hasExtra("usb_port")) UsbReplay(holder.surface, active).run(intent.getIntExtra("usb_port", 0))
            else replay(holder)
        } catch (error: Exception) { JSONObject().put("error", error.stackTraceToString()) }
        result.put("display_hz", windowManager.defaultDisplay.refreshRate)
        File(filesDir, "result.json").writeText(result.toString(2))
        runOnUiThread { finish() }
    }
    private fun replay(holder: SurfaceHolder): JSONObject {
            val profile = intent.getStringExtra("profile") ?: "legacy"
            val seconds = intent.getIntExtra("seconds", 30).coerceIn(1, 600)
            val warmup = intent.getIntExtra("warmup", 5).coerceIn(0, 60)
            val rate = intent.getIntExtra("rate", 60).coerceIn(1, 90)
            val burst = intent.getIntExtra("burst", 1).coerceIn(1, 32)
            val selection = intent.getStringExtra("selection")?.let {
                JSONObject(String(android.util.Base64.decode(it, android.util.Base64.DEFAULT), Charsets.UTF_8))
            }
            return DecoderReplay(holder.surface, profile, active, burst, selection).run(ReplayClip(File(filesDir, "stream.bin")), rate, seconds, warmup)
    }
    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {}
    override fun surfaceDestroyed(holder: SurfaceHolder) { active.set(false) }
    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (!hasFocus && worker != null) active.set(false)
    }
    override fun dispatchTouchEvent(event: android.view.MotionEvent): Boolean {
        if (event.actionMasked == android.view.MotionEvent.ACTION_DOWN) active.set(false)
        return super.dispatchTouchEvent(event)
    }
    override fun onPause() { active.set(false); super.onPause() }
}
