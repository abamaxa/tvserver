package com.abamaxa.lots_of_videos

import android.annotation.SuppressLint
import android.app.Service
import android.content.Intent
import android.os.IBinder
import android.os.PowerManager

/** Android 7–13 fallback. The screen may sleep while the transfer CPU work continues. */
class NativeDownloadService : Service() {
    private var wakeLock: PowerManager.WakeLock? = null

    override fun onBind(intent: Intent?): IBinder? = null

    @SuppressLint("WakelockTimeout")
    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (NativeDownloads.entries().isEmpty()) {
            stopSelf(startId)
            return START_NOT_STICKY
        }
        startForeground(NativeDownloads.LEGACY_NOTIFICATION, NativeDownloads.notification())
        if (wakeLock == null) {
            wakeLock = getSystemService(PowerManager::class.java)
                .newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "$packageName:downloads").apply { acquire() }
        }
        NativeDownloads.legacyStarted(this)
        return START_NOT_STICKY
    }

    fun refresh() {
        if (NativeDownloads.entries().isEmpty()) {
            wakeLock?.let { if (it.isHeld) it.release() }
            wakeLock = null
            stopForeground(STOP_FOREGROUND_REMOVE)
            stopSelf()
        } else startForeground(NativeDownloads.LEGACY_NOTIFICATION, NativeDownloads.notification())
    }

    override fun onTimeout(startId: Int, fgsType: Int) {
        NativeDownloads.stopAll("Android stopped this download; retry it from Tasks")
        stopSelf()
    }

    override fun onDestroy() {
        wakeLock?.let { if (it.isHeld) it.release() }
        wakeLock = null
        NativeDownloads.legacyStopped(this)
        super.onDestroy()
    }
}
