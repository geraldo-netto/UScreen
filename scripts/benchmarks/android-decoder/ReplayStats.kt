package com.uscreen.benchmark

import android.os.Debug
import android.os.Process
import android.os.SystemClock
import java.io.File
import org.json.JSONArray
import org.json.JSONObject

internal class ReplayStats {
    private val samples = ArrayList<Int>()
    private var rendered = 0
    private var invalidations = 0
    private var measuring = false
    private var firstSequence = 0
    private val seen = HashSet<Int>()
    private var duplicates = 0
    @Synchronized fun rendered(sequence: Int, micros: Int) {
        if (!measuring || sequence < firstSequence) return
        if (!seen.add(sequence)) { duplicates++; return }
        rendered++
        if (samples.size < 36_000 && micros >= 0) samples.add(micros)
    }
    @Synchronized fun invalidated() { invalidations++ }
    @Synchronized fun begin(sequence: Int) { samples.clear(); seen.clear(); rendered = 0; firstSequence = sequence; measuring = true }
    @Synchronized fun count(): Int = rendered
    @Synchronized fun finish(): JSONObject {
        measuring = false
        return JSONObject().put("rendered", rendered).put("invalidations", invalidations)
            .put("duplicates", duplicates).put("arrival_to_callback_us", JSONArray(samples))
    }
    companion object {
        fun process(): JSONObject = JSONObject().put("elapsed_ns", System.nanoTime())
            .put("process_cpu_ms", Process.getElapsedCpuTime()).put("uptime_ms", SystemClock.uptimeMillis())
            .put("runtime", runtime()).put("threads", threads())

        private fun runtime(): JSONObject = JSONObject().apply {
            Debug.getRuntimeStats().forEach { (key, value) -> put(key, value) }
        }

        private fun threads(): JSONArray {
            val rows = JSONArray()
            File("/proc/self/task").listFiles()?.forEach { task ->
                try { rows.put(thread(task)) } catch (_: Exception) {} // A worker can retire during sampling.
            }
            return rows
        }
        private fun thread(task: File): JSONObject {
            val row = JSONObject().put("tid", task.name)
            val wanted = setOf("Name", "voluntary_ctxt_switches", "nonvoluntary_ctxt_switches")
            File(task, "status").forEachLine { line ->
                val pair = line.split(':', limit = 2)
                if (pair[0] in wanted) row.put(pair[0], pair[1].trim())
            }
            return row
        }
    }
}
