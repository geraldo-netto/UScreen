package com.blent

import android.app.*
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.*

class MicrophoneService : AudioForegroundService(1)
class SpeakerService : AudioForegroundService(2)

open class AudioForegroundService(private val direction: Int) : Service() {
    private val label = if (direction == 1) "microphone" else "speakers"
    private val notificationId = 717 + direction
    private var run: String? = null
    private var wake: PowerManager.WakeLock? = null
    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val requested = intent?.getStringExtra("audio_run")
        if (intent?.action == "stop_audio") {
            AudioOwner.stopped(requested, direction)
            if (!AudioOwner.owns(run, direction)) stopSelf()
            return START_NOT_STICKY
        }
        if (!AudioOwner.owns(requested, direction)) { if (!AudioOwner.owns(run, direction)) stopSelf(); return START_NOT_STICKY }
        run = requested
        try { foreground(); acquireWake() } catch (_: Exception) { AudioOwner.stopped(run, direction); stopSelf() }
        return START_NOT_STICKY
    }
    private fun foreground() {
        val manager = getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(NotificationChannel("blent_$label", "Blent $label", NotificationManager.IMPORTANCE_LOW))
        val stop = PendingIntent.getService(this, notificationId, Intent(this, javaClass).setAction("stop_audio")
            .setData(android.net.Uri.parse("blent://$label/$run")).putExtra("audio_run", run), PendingIntent.FLAG_IMMUTABLE)
        val notification = Notification.Builder(this, "blent_$label").setContentTitle("Blent $label sharing")
            .setContentText("Your computer can use these audio devices")
            .setSmallIcon(android.R.drawable.ic_btn_speak_now)
            .addAction(Notification.Action.Builder(null, "Stop $label", stop).build())
            .setContentIntent(PendingIntent.getActivity(this, notificationId, Intent(this, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE))
            .setOngoing(true).build()
        val typed = if (direction == 1) Build.VERSION.SDK_INT >= 30 else Build.VERSION.SDK_INT >= 29
        if (typed) startForeground(notificationId, notification, if (direction == 1) ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE else ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK)
        else startForeground(notificationId, notification)
    }
    @android.annotation.SuppressLint("WakelockTimeout")
    private fun acquireWake() {
        if (wake != null) return
        wake = getSystemService(PowerManager::class.java).newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "Blent::Audio-$direction").apply {
            setReferenceCounted(false); acquire()
        }
    }
    override fun onDestroy() {
        wake?.let { if (it.isHeld) it.release() }; wake = null
        AudioOwner.stopped(run, direction)
        super.onDestroy()
    }
    override fun onBind(intent: Intent?): IBinder? = null
}
