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
            val profile = intent.getStringExtra("profile") ?: "legacy"
            val seconds = intent.getIntExtra("seconds", 30).coerceIn(1, 600)
            val warmup = intent.getIntExtra("warmup", 5).coerceIn(0, 60)
            val rate = intent.getIntExtra("rate", 60).coerceIn(1, 90)
            DecoderReplay(holder.surface, profile, active).run(ReplayClip(File(filesDir, "stream.bin")), rate, seconds, warmup)
        } catch (error: Exception) { JSONObject().put("error", error.stackTraceToString()) }
        result.put("display_hz", windowManager.defaultDisplay.refreshRate)
        File(filesDir, "result.json").writeText(result.toString(2))
        runOnUiThread { finish() }
    }
    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {}
    override fun surfaceDestroyed(holder: SurfaceHolder) { active.set(false) }
    override fun onPause() { active.set(false); super.onPause() }
}
