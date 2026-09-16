package com.uscreen

import android.util.Log
import okhttp3.OkHttpClient
import okhttp3.Request
import org.json.JSONObject
import java.util.concurrent.TimeUnit

/**
 * "A newer release exists", and nothing more. Sideloaded apps cannot update
 * themselves silently — Android always asks the user — so this just answers
 * the question and hands over the release page.
 */
object UpdateCheck {
    private const val TAG = "UScreenUpdate"
    const val RELEASES_PAGE = "https://github.com/majmichu1/UScreen/releases/latest"
    private const val API = "https://api.github.com/repos/majmichu1/UScreen/releases/latest"

    private val client = OkHttpClient.Builder()
        .connectTimeout(10, TimeUnit.SECONDS)
        .readTimeout(10, TimeUnit.SECONDS)
        .build()

    private data class Version(val core: List<Long>, val pre: List<String>)
    private val syntax = Regex("""(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?""")

    private fun parse(value: String): Version? {
        val match = syntax.matchEntire(value.trim().removePrefix("v")) ?: return null
        val core = (1..3).map { index ->
            match.groupValues[index].toLongOrNull()?.takeIf { it <= 0xffff_ffffL } ?: return null
        }
        val pre = match.groupValues[4].takeIf { it.isNotEmpty() }?.split('.') ?: emptyList()
        if (pre.any { it.all(Char::isDigit) && it.length > 1 && it.startsWith('0') }) return null
        return Version(core, pre)
    }

    fun isNewer(candidate: String, current: String): Boolean {
        val a = parse(candidate) ?: return false
        val b = parse(current) ?: return false
        for (i in 0..2) if (a.core[i] != b.core[i]) return a.core[i] > b.core[i]
        if (a.pre.isEmpty() || b.pre.isEmpty()) return a.pre.isEmpty() && b.pre.isNotEmpty()
        for (i in 0 until minOf(a.pre.size, b.pre.size)) {
            val x = a.pre[i]; val y = b.pre[i]
            if (x == y) continue
            val xn = x.all(Char::isDigit); val yn = y.all(Char::isDigit)
            if (xn != yn) return !xn
            if (xn && x.length != y.length) return x.length > y.length
            return x > y
        }
        return a.pre.size > b.pre.size
    }

    /** Blocking; call off the main thread. Returns the newer version or null. */
    fun newerThan(current: String): String? {
        return try {
            val req = Request.Builder().url(API)
                .header("Accept", "application/vnd.github+json")
                .header("User-Agent", "uscreen-android/$current")
                .build()
            client.newCall(req).execute().use { resp ->
                if (!resp.isSuccessful) return null
                val tag = JSONObject(resp.body?.string() ?: return null)
                    .optString("tag_name")
                if (isNewer(tag, current)) tag.trim().removePrefix("v") else null
            }
        } catch (e: Exception) {
            Log.d(TAG, "update check skipped: ${e.message}"); null
        }
    }
}
