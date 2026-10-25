package com.abamaxa.lots_of_videos

import android.app.job.JobParameters
import android.app.job.JobService
import androidx.annotation.RequiresApi

/** JobScheduler holds the CPU/network lifetime for user-started downloads on Android 14+. */
@RequiresApi(34)
class NativeDownloadJobService : JobService() {
    override fun onStartJob(params: JobParameters): Boolean = NativeDownloads.jobStarted(this, params)

    override fun onStopJob(params: JobParameters): Boolean {
        NativeDownloads.jobStopped(params)
        return false
    }
}
