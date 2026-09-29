package com.blent

import android.app.*
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.*

class MicrophoneService : Service() {
    private var run: String? = null
    private var wake: PowerManager.WakeLock? = null
    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val requested = intent?.getStringExtra("audio_run")
        if (intent?.action == "stop_audio") {
            AudioOwner.stopped(requested)
            if (!AudioOwner.owns(run)) stopSelf()
            return START_NOT_STICKY
        }
        if (!AudioOwner.owns(requested)) { if (!AudioOwner.owns(run)) stopSelf(); return START_NOT_STICKY }
        run = requested
        try { foreground(); acquireWake() } catch (_: Exception) { AudioOwner.stopped(run); stopSelf() }
        return START_NOT_STICKY
    }
    private fun foreground() {
        val manager = getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(NotificationChannel("blent_microphone", "Blent microphone", NotificationManager.IMPORTANCE_LOW))
        val stop = PendingIntent.getService(this, 718, Intent(this, MicrophoneService::class.java).setAction("stop_audio")
            .setData(android.net.Uri.parse("blent://microphone/$run")).putExtra("audio_run", run), PendingIntent.FLAG_IMMUTABLE)
        val notification = Notification.Builder(this, "blent_microphone").setContentTitle("Blent microphone sharing")
            .setContentText("Your computer can use this microphone")
            .setSmallIcon(android.R.drawable.ic_btn_speak_now)
            .addAction(Notification.Action.Builder(null, "Stop microphone", stop).build())
            .setContentIntent(PendingIntent.getActivity(this, 718, Intent(this, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE))
            .setOngoing(true).build()
        if (Build.VERSION.SDK_INT >= 30) startForeground(718, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE)
        else startForeground(718, notification)
    }
    @android.annotation.SuppressLint("WakelockTimeout")
    private fun acquireWake() {
        if (wake != null) return
        wake = getSystemService(PowerManager::class.java).newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "Blent::Microphone").apply {
            setReferenceCounted(false); acquire()
        }
    }
    override fun onDestroy() {
        wake?.let { if (it.isHeld) it.release() }; wake = null
        AudioOwner.stopped(run)
        super.onDestroy()
    }
    override fun onBind(intent: Intent?): IBinder? = null
}
