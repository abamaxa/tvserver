package com.abamaxa.lots_of_videos

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.job.JobInfo
import android.app.job.JobParameters
import android.app.job.JobScheduler
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.PersistableBundle
import android.text.format.Formatter
import android.util.Log
import androidx.annotation.RequiresApi
import org.json.JSONObject
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger

/** Main-thread owner of Android transfer jobs. The Rust engines own the actual bytes. */
internal object NativeDownloads {
    const val CHANNEL = "downloads"
    const val LEGACY_NOTIFICATION = 7100
    const val TOKEN = "downloadToken"
    private val nextNotification = AtomicInteger(7200)
    private val main = Handler(Looper.getMainLooper())
    private val active = linkedMapOf<String, Entry>()
    private var context: Context? = null
    var legacyService: NativeDownloadService? = null
        private set

    class Entry(val id: String, val name: String) {
        val notificationId = nextNotification.getAndIncrement()
        val ready = CountDownLatch(1)
        @Volatile var failure: String? = null
        var service: NativeDownloadJobService? = null
        var parameters: JobParameters? = null
        var downloaded = 0L
        var total: Long? = null
    }

    // Called from Rust's blocking pool, never Android's main thread. Wait for the
    // OS to grant execution before letting Python or torrent initialization start.
    fun begin(appContext: Context, payload: String): String {
        check(Looper.myLooper() != Looper.getMainLooper()) { "Download startup must run off the main thread" }
        val request = JSONObject(payload)
        val entry = Entry(request.getString("id"), request.getString("name"))
        main.post {
            context = appContext.applicationContext
            try {
                check(!active.containsKey(entry.id)) { "Download is already running" }
                active[entry.id] = entry
                createChannel(appContext)
                if (Build.VERSION.SDK_INT >= 34) {
                    val extras = PersistableBundle().apply { putString(TOKEN, entry.id) }
                    val job = JobInfo.Builder(entry.notificationId,
                        ComponentName(appContext, NativeDownloadJobService::class.java))
                        .setUserInitiated(true)
                        .setRequiredNetworkType(JobInfo.NETWORK_TYPE_ANY)
                        .setExtras(extras)
                        .build()
                    check(appContext.getSystemService(JobScheduler::class.java).schedule(job) == JobScheduler.RESULT_SUCCESS) {
                        "Android could not start the background download; keep the app open and retry"
                    }
                } else {
                    val intent = Intent(appContext, NativeDownloadService::class.java)
                    if (Build.VERSION.SDK_INT >= 26) appContext.startForegroundService(intent)
                    else appContext.startService(intent)
                }
            } catch (error: Exception) {
                entry.failure = error.message ?: "Could not start background download"
                finish(entry.id)
                entry.ready.countDown()
            }
        }
        if (!entry.ready.await(20, TimeUnit.SECONDS)) {
            entry.failure = "Android did not start the background download; keep the app open and retry"
            main.post { finish(entry.id) }
        }
        return entry.failure?.let { JSONObject().put("error", it).toString() } ?: "{}"
    }

    fun update(payload: String) {
        val update = JSONObject(payload)
        main.post {
            val entry = active[update.getString("id")] ?: return@post
            entry.downloaded = update.optLong("downloaded").coerceAtLeast(0)
            entry.total = if (update.isNull("total")) null else update.optLong("total").takeIf { it > 0 }
            if (Build.VERSION.SDK_INT >= 34) {
                val parameters = entry.parameters ?: return@post
                entry.service?.setNotification(parameters, entry.notificationId, notification(entry),
                    android.app.job.JobService.JOB_END_NOTIFICATION_POLICY_REMOVE)
            } else legacyService?.refresh()
        }
    }

    fun end(payload: String) {
        val id = JSONObject(payload).getString("id")
        main.post { finish(id) }
    }

    @RequiresApi(34)
    fun jobStarted(service: NativeDownloadJobService, parameters: JobParameters): Boolean {
        val entry = active[parameters.extras.getString(TOKEN)] ?: return false
        if (parameters.jobId != entry.notificationId) return false
        try {
            service.setNotification(parameters, entry.notificationId, notification(entry),
                android.app.job.JobService.JOB_END_NOTIFICATION_POLICY_REMOVE)
            entry.service = service
            entry.parameters = parameters
            entry.ready.countDown()
            return true
        } catch (error: Exception) {
            entry.failure = error.message ?: "Could not show background download"
            finish(entry.id, stoppedJob = parameters)
            return false
        }
    }

    @RequiresApi(34)
    fun jobStopped(parameters: JobParameters) {
        val entry = active[parameters.extras.getString(TOKEN)] ?: return
        if (entry.parameters?.jobId != parameters.jobId) return
        Log.w("LotsOfVideosDownloads", "Android stopped a download job: ${parameters.stopReason}")
        stop(entry.id, "Android stopped this download; retry it from Tasks", parameters)
    }

    fun stop(id: String, reason: String, stoppedJob: JobParameters? = null) {
        val entry = active[id] ?: return
        entry.failure = reason
        NativeBridge.downloadStopped(id, reason)
        finish(id, stoppedJob)
    }

    fun stopAll(reason: String) {
        active.keys.toList().forEach { stop(it, reason) }
    }

    private fun finish(id: String, stoppedJob: JobParameters? = null) {
        val entry = active.remove(id) ?: return
        if (entry.ready.count > 0) entry.failure = entry.failure ?: "Download cancelled before it started"
        entry.ready.countDown()
        if (Build.VERSION.SDK_INT >= 34) {
            val parameters = entry.parameters
            if (parameters != null && stoppedJob == null) entry.service?.jobFinished(parameters, false)
            else if (stoppedJob == null) context?.getSystemService(JobScheduler::class.java)?.cancel(entry.notificationId)
            context?.getSystemService(NotificationManager::class.java)?.cancel(entry.notificationId)
        } else legacyService?.refresh()
    }

    fun legacyStarted(service: NativeDownloadService) {
        legacyService = service
        active.values.forEach { it.ready.countDown() }
    }

    fun legacyStopped(service: NativeDownloadService) {
        if (legacyService !== service) return
        legacyService = null
        if (active.isNotEmpty()) stopAll("Android stopped this download; retry it from Tasks")
    }

    fun entries(): List<Entry> = active.values.toList()

    private fun createChannel(context: Context) {
        if (Build.VERSION.SDK_INT >= 26) {
            context.getSystemService(NotificationManager::class.java).createNotificationChannel(
                NotificationChannel(CHANNEL, "Downloads", NotificationManager.IMPORTANCE_LOW))
        }
    }

    fun notification(entry: Entry? = null): Notification {
        val appContext = checkNotNull(context)
        val entries = active.values
        val single = entry ?: entries.singleOrNull()
        val downloaded = single?.downloaded ?: entries.sumOf { it.downloaded }
        val total = single?.total
        val text = when {
            total != null -> "${Formatter.formatFileSize(appContext, downloaded)} of ${Formatter.formatFileSize(appContext, total)}"
            downloaded > 0 -> "${Formatter.formatFileSize(appContext, downloaded)} downloaded"
            else -> "Starting download…"
        }
        val open = PendingIntent.getActivity(appContext, 0,
            Intent(appContext, StorageLauncherActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
        val cancel = PendingIntent.getBroadcast(appContext, entry?.notificationId ?: LEGACY_NOTIFICATION,
            Intent(appContext, NativeDownloadCancelReceiver::class.java).apply {
                if (entry != null) putExtra(TOKEN, entry.id)
            }, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
        val builder = if (Build.VERSION.SDK_INT >= 26) Notification.Builder(appContext, CHANNEL)
            else @Suppress("DEPRECATION") Notification.Builder(appContext)
        return builder.setSmallIcon(android.R.drawable.stat_sys_download)
            .setContentTitle(single?.name ?: "${entries.size} downloads")
            .setContentText(text)
            .setSubText("Lots of Videos downloads")
            .setContentIntent(open)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setCategory(Notification.CATEGORY_PROGRESS)
            .setProgress(100, if (total != null) ((downloaded.toDouble() / total) * 100).toInt().coerceIn(0, 100) else 0, total == null)
            .addAction(Notification.Action.Builder(null, if (entry != null) "Cancel" else "Cancel downloads", cancel).build())
            .build()
    }
}
