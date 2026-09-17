package com.uscreen

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.os.Build
import android.os.IBinder
import android.net.wifi.WifiManager
import android.os.PowerManager
import android.util.Log

class StreamingService : Service() {
    companion object {
        const val CHANNEL_ID = "uscreen_streaming"
        const val NOTIFICATION_ID = 1
        private const val TAG = "UScreenService"
    }

    private var wakeLock: PowerManager.WakeLock? = null
    private var wifiLock: WifiManager.WifiLock? = null

    override fun onCreate() {
        super.onCreate()
        createNotificationChannel()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val notificationIntent = Intent(this, MainActivity::class.java)
        val pendingIntent = PendingIntent.getActivity(
            this, 0, notificationIntent,
            PendingIntent.FLAG_IMMUTABLE
        )

        val notification = Notification.Builder(this, CHANNEL_ID)
            .setContentTitle("UScreen Active")
            .setContentText("Streaming display to this device")
            .setSmallIcon(android.R.drawable.ic_menu_view)
            .setContentIntent(pendingIntent)
            .setOngoing(true)
            .build()

        // Never let this kill the app. The foreground service only helps the
        // process survive backgrounding — streaming itself does not depend on
        // it, so a platform rejection (the FGS type rules change between
        // Android releases) must degrade, not crash.
        try {
            startForeground(NOTIFICATION_ID, notification)
        } catch (e: Exception) {
            android.util.Log.e(
                "UScreenService",
                "startForeground rejected: ${e.message}. Continuing without it.",
                e
            )
            stopSelf()
            return START_NOT_STICKY
        }

        // Acquire a partial wake lock to prevent CPU sleep.
        //
        // Guarded against re-entry: onStartCommand runs again on every
        // startService call, and the previous version replaced the field each
        // time without releasing the old lock, leaking one per call.
        if (wakeLock?.isHeld != true) {
            val pm = getSystemService(POWER_SERVICE) as PowerManager
            wakeLock = pm.newWakeLock(
                PowerManager.PARTIAL_WAKE_LOCK,
                "UScreen::StreamingWakeLock"
            ).apply {
                setReferenceCounted(false)
                acquire(4 * 60 * 60 * 1000L) // 4 hours max
            }
        }

        // Keep the Wi-Fi radio out of power save.
        //
        // Upstream Wi-Fi measurements found large latency spikes from radio
        // power saving. This lock can help that transport, but its effect is
        // device-dependent. An active Wi-Fi connection can still be affected
        // while our ADB stream uses USB; T388 tracks power/transport measurements.
        //
        // LOW_LATENCY over HIGH_PERF: it also asks the driver for a
        // low-latency mode. LOW_LATENCY was added in API 29; HIGH_PERF was
        // deprecated in API 34. LOW_LATENCY only takes effect while connected
        // to an access point, and it only
        // applies while the screen is on and this app is foreground, which is
        // precisely when frames are arriving.
        if (wifiLock?.isHeld != true) {
            try {
                val wm = applicationContext.getSystemService(WIFI_SERVICE) as WifiManager
                val mode = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                    WifiManager.WIFI_MODE_FULL_LOW_LATENCY
                } else {
                    @Suppress("DEPRECATION")
                    WifiManager.WIFI_MODE_FULL_HIGH_PERF
                }
                wifiLock = wm.createWifiLock(mode, "UScreen::StreamingWifiLock").apply {
                    setReferenceCounted(false)
                    acquire()
                }
                Log.i(TAG, "Wi-Fi lock held (low latency)")
            } catch (e: Exception) {
                // Not fatal: it only costs latency on a wireless link.
                Log.w(TAG, "Could not take a Wi-Fi lock: ${e.message}")
            }
        }

        // Not sticky: this service does no work of its own, it only holds
        // locks while the activity streams. Restarted by the system after
        // process death, with no activity behind it, it would hold a wake
        // lock and a Wi-Fi lock for hours for nothing.
        return START_NOT_STICKY
    }

    override fun onDestroy() {
        wakeLock?.let {
            if (it.isHeld) it.release()
        }
        wifiLock?.let {
            if (it.isHeld) it.release()
        }
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    private fun createNotificationChannel() {
        val channel = NotificationChannel(
            CHANNEL_ID,
            "UScreen Streaming",
            NotificationManager.IMPORTANCE_LOW
        ).apply {
            description = "Keeps UScreen alive while streaming"
            setShowBadge(false)
        }
        val nm = getSystemService(NotificationManager::class.java)
        nm.createNotificationChannel(channel)
    }
}
