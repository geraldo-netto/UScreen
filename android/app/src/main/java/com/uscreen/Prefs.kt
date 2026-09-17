package com.uscreen

import android.content.Context
import android.content.SharedPreferences

/** Persisted user settings on the tablet side. */
class Prefs(context: Context) {
    private val sp: SharedPreferences =
        context.getSharedPreferences("uscreen", Context.MODE_PRIVATE)

    companion object {
        /**
         * Defaults deliberately match the host's own defaults. They used to be
         * 200 Mbps / 90 fps, which the app pushed to the host on every start —
         * overwriting whatever was configured on the desktop and driving the
         * encoder far past what the USB link carries, so frames queued up and
         * latency grew without bound.
         */
        const val DEFAULT_BITRATE_KBPS = 20_000
        const val DEFAULT_FPS = 60
        const val DEFAULT_BRIGHTNESS_PERCENT = 50
        const val DEFAULT_DISPLAY_REFRESH_RATE = 60f

        /** Kept in sync with `config::MAX_BITRATE_KBPS` on the host. */
        const val MAX_BITRATE_KBPS = 60_000
        const val MIN_BITRATE_KBPS = 1_000

        /** Values of [orientation]. */
        const val ORIENTATION_AUTO = 0
        const val ORIENTATION_CAMERA_UP = 1
        const val ORIENTATION_CAMERA_DOWN = 2
    }

    var bitrateKbps: Int
        get() = sp.getInt("bitrate_kbps", DEFAULT_BITRATE_KBPS)
            .coerceIn(MIN_BITRATE_KBPS, MAX_BITRATE_KBPS)
        set(v) = sp.edit().putInt("bitrate_kbps", v).apply()

    var fps: Int
        get() = sp.getInt("fps", DEFAULT_FPS)
        set(v) = sp.edit().putInt("fps", v).apply()

    /** Local window preferences, independent of the host's stream configuration. */
    var brightnessPercent: Int
        get() = sp.getInt("brightness_percent", DEFAULT_BRIGHTNESS_PERCENT).coerceIn(0, 100)
        set(v) = sp.edit().putInt("brightness_percent", v.coerceIn(0, 100)).apply()

    /** Zero follows Android's system preference. */
    var displayRefreshRate: Float
        get() = sp.getFloat("display_refresh_rate", DEFAULT_DISPLAY_REFRESH_RATE)
            .takeIf { it.isFinite() && it >= 0f } ?: DEFAULT_DISPLAY_REFRESH_RATE
        set(v) = sp.edit().putFloat("display_refresh_rate", v).apply()

    var showStats: Boolean
        get() = sp.getBoolean("show_stats", false)
        set(v) = sp.edit().putBoolean("show_stats", v).apply()

    /**
     * Which way round the tablet is held: [ORIENTATION_AUTO] follows the
     * sensor between the two landscape directions, the other two pin it.
     * Pinning exists because the sensor path does not work everywhere — a
     * Galaxy Tab S9 Ultra with auto-rotate on never left "camera up" — and
     * because people who draw with the camera at the bottom do not want the
     * picture flipping when the tablet is lifted.
     */
    var orientation: Int
        get() = sp.getInt("orientation", ORIENTATION_AUTO)
        set(v) = sp.edit().putInt("orientation", v).apply()

    /**
     * True once the user has actually applied settings from the sheet.
     *
     * Until then the tablet stays silent instead of pushing its defaults on
     * every connect: the desktop GUI is the source of truth, and a tablet that
     * announces stale defaults at startup silently undoes whatever was set
     * there. Only a deliberate "Apply" gives the tablet the right to speak.
     */
    var hasUserSettings: Boolean
        get() = sp.getBoolean("has_user_settings", false)
        set(v) = sp.edit().putBoolean("has_user_settings", v).apply()

    /**
     * The daemon's session token, delivered as an intent extra when it
     * launches the app over adb. Kept so a relaunch by hand within the same
     * daemon run still authenticates; a new daemon run hands out a new one.
     */
    var hostToken: String?
        get() = sp.getString("host_token", null)
        set(v) = sp.edit().putString("host_token", v).apply()

    /**
     * Whether to ask GitHub for a newer release when the app comes to the
     * front. The host has `check_updates` for the daemon and GUI; this is the
     * tablet's own switch, so the app can be kept fully offline too.
     */
    var checkUpdates: Boolean
        get() = sp.getBoolean("check_updates", true)
        set(v) = sp.edit().putBoolean("check_updates", v).apply()

    /** Shown once, after the first time video actually arrived. */
    var thankedOnce: Boolean
        get() = sp.getBoolean("thanked_once", false)
        set(v) = sp.edit().putBoolean("thanked_once", v).apply()
}
