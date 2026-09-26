package com.blent

import android.app.*
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.*

/** Keeps an explicitly started camera alive while the Activity is hidden/locked. */
class CameraService : Service() {
    private var wake: PowerManager.WakeLock? = null

    override fun onCreate() {
        super.onCreate()
        try {
            val manager = getSystemService(NotificationManager::class.java)
            manager.createNotificationChannel(NotificationChannel("blent_camera", "Blent camera", NotificationManager.IMPORTANCE_LOW))
            val notification = Notification.Builder(this, "blent_camera")
                .setContentTitle("Blent camera sharing")
                .setContentText("Camera controlled by Blent on your computer")
                .setSmallIcon(android.R.drawable.ic_menu_camera)
                .setContentIntent(PendingIntent.getActivity(this, 0, Intent(this, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE))
                .setOngoing(true).build()
            if (Build.VERSION.SDK_INT >= 30) startForeground(2, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_CAMERA)
            else startForeground(2, notification)
            acquireWake()
        } catch (_: Exception) {
            CameraOwner.serviceStopped()
            stopSelf()
        }
    }

    @android.annotation.SuppressLint("WakelockTimeout")
    private fun acquireWake() {
        wake = getSystemService(PowerManager::class.java).newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "Blent::Camera").apply {
            setReferenceCounted(false)
            acquire()
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent == null) stopSelf()
        return START_NOT_STICKY
    }

    override fun onDestroy() {
        wake?.let { if (it.isHeld) it.release() }
        CameraOwner.serviceStopped()
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null
}
