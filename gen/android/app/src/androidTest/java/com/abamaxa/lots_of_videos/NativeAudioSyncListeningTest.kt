package com.abamaxa.lots_of_videos

import android.content.Intent
import android.os.SystemClock
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONObject
import org.junit.Assume.assumeTrue
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File

/** Opt-in listening probe; uses only a sample explicitly copied into test-app cache. */
@RunWith(AndroidJUnit4::class)
class NativeAudioSyncListeningTest {
    @Test fun playSuppliedSample() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val arguments = InstrumentationRegistry.getArguments()
        assumeTrue("Listening probe was not requested", arguments.getString("listen") == "true")
        val context = instrumentation.targetContext
        val sample = File(context.cacheDir, "sync-listening-sample.mp4")
        check(sample.isFile) { "Copy a listening sample into the isolated test app's cache first" }
        val request = JSONObject().put("videoId", "sync-listening-sample")
            .put("video", "Audio sync listening check").put("collection", "Temporary test copy")
            .put("path", sample.absolutePath).put("startTime", arguments.getString("startTime", "0").toDouble())
        val intent = Intent(context, NativePlayerActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            .putExtra("video", request.toString())
        ActivityScenario.launch<NativePlayerActivity>(intent).use {
            val deadline = SystemClock.elapsedRealtime() + 180000
            val done = File(context.cacheDir, "sync-listening-done")
            try {
                while (SystemClock.elapsedRealtime() < deadline && !done.exists()) SystemClock.sleep(250)
            } finally {
                instrumentation.runOnMainSync { NativeBridge.playback?.stopPlayback() }
                done.delete()
            }
        }
    }
}
