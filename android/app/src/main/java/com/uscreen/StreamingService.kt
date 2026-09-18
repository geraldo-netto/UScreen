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

    private val handler = android.os.Handler(android.os.Looper.getMainLooper())
    private var releasePending = false
    private val releaseIdleWifi = Runnable { releasePending = false; releaseWifi() }

    override fun onCreate() {
        super.onCreate()
        createNotificationChannel()
        promote() // Satisfy the foreground-start contract before waiting for commands.
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent == null || !promote()) {
            releaseLocks()
            stopSelf()
            return START_NOT_STICKY
        }
        applyPower(StreamingPower.read(intent))
        return START_NOT_STICKY // No useful work without the owning Activity.
    }

    private fun promote(): Boolean = try {
        startForeground(NOTIFICATION_ID, notification())
        true
    } catch (error: Exception) {
        Log.w(TAG, "Foreground promotion rejected", error)
        releaseLocks()
        stopSelf()
        false
    }

    private fun notification(): Notification {
        val pending = PendingIntent.getActivity(this, 0, Intent(this, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE)
        return Notification.Builder(this, CHANNEL_ID)
            .setContentTitle("UScreen Active").setContentText("Streaming display to this device")
            .setSmallIcon(android.R.drawable.ic_menu_view).setContentIntent(pending).setOngoing(true).build()
    }

    private fun applyPower(power: StreamingPower) {
        // The visible Activity already holds FLAG_KEEP_SCREEN_ON. Preserve the
        // original locks in normal mode; battery mode needs no extra CPU lock.
        if (power.batterySaver) releaseWake() else acquireWake()
        if (power.needsWifi) {
            cancelIdleRelease()
            acquireWifi()
        } else if (power.transport == StreamTransport.USB) {
            cancelIdleRelease()
            releaseWifi()
        } else if (wifiLock?.isHeld == true && !releasePending) {
            releasePending = true
            handler.postDelayed(releaseIdleWifi, 5000)
        }
    }

    private fun acquireWake() {
        if (wakeLock?.isHeld == true) return
        try {
            val manager = getSystemService(POWER_SERVICE) as PowerManager
            wakeLock = manager.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "UScreen::StreamingWakeLock").apply {
                setReferenceCounted(false)
                acquire(4 * 60 * 60 * 1000L)
            }
        } catch (error: Exception) { Log.w(TAG, "CPU lock unavailable", error) }
    }

    private fun acquireWifi() {
        if (wifiLock?.isHeld == true) return
        try {
            val manager = applicationContext.getSystemService(WIFI_SERVICE) as WifiManager
            val mode = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) WifiManager.WIFI_MODE_FULL_LOW_LATENCY
                else @Suppress("DEPRECATION") WifiManager.WIFI_MODE_FULL_HIGH_PERF
            wifiLock = manager.createWifiLock(mode, "UScreen::StreamingWifiLock").apply {
                setReferenceCounted(false)
                acquire()
            }
        } catch (error: Exception) { Log.w(TAG, "Wi-Fi lock unavailable", error) }
    }

    private fun cancelIdleRelease() {
        handler.removeCallbacks(releaseIdleWifi)
        releasePending = false
    }
    private fun releaseWake() { wakeLock?.let { if (it.isHeld) it.release() } }
    private fun releaseWifi() { wifiLock?.let { if (it.isHeld) it.release() } }
    private fun releaseLocks() { cancelIdleRelease(); releaseWake(); releaseWifi() }

    override fun onDestroy() {
        releaseLocks()
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
