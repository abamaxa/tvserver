package com.abamaxa.lots_of_videos

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

class NativeDownloadCancelReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val id = intent.getStringExtra(NativeDownloads.TOKEN)
        if (id != null) NativeDownloads.stop(id, "Download cancelled")
        else NativeDownloads.stopAll("Download cancelled")
    }
}
