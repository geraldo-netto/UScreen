package com.uscreen

import android.util.Log
import okhttp3.Call
import okhttp3.Callback
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.Response
import org.json.JSONObject
import java.io.IOException
import java.util.concurrent.TimeUnit

internal class HttpReleaseChecks(
    private val endpoint: String,
    client: OkHttpClient = OkHttpClient(),
    deadlineMillis: Long = 15_000,
) : ReleaseChecks {
    private val client = client.newBuilder()
        .connectTimeout(10, TimeUnit.SECONDS)
        .readTimeout(10, TimeUnit.SECONDS)
        .callTimeout(deadlineMillis, TimeUnit.MILLISECONDS)
        .build()

    override fun newCall(current: String): ReleaseCall {
        val request = Request.Builder().url(endpoint)
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "uscreen-android/$current").build()
        return HttpReleaseCall(client.newCall(request), current)
    }
}

private class HttpReleaseCall(private val call: Call, private val current: String) : ReleaseCall {
    override fun cancel() = call.cancel()
    override fun start(result: (String?) -> Unit) {
        call.enqueue(object : Callback {
            override fun onFailure(call: Call, e: IOException) { result(null) }
            override fun onResponse(call: Call, response: Response) {
                val found = try { response.use { newer(it) } } catch (error: Exception) {
                    Log.d("UScreenUpdate", "update check skipped: ${error.message}")
                    null
                }
                result(found)
            }
        })
    }
    private fun newer(response: Response): String? {
        if (!response.isSuccessful) return null
        val tag = JSONObject(response.body?.string() ?: return null).optString("tag_name")
        return if (UpdateCheck.isNewer(tag, current)) tag.trim().removePrefix("v") else null
    }
}
