package com.abamaxa.lots_of_videos

import android.Manifest
import android.app.NotificationManager
import android.app.job.JobScheduler
import android.content.Intent
import android.os.Build
import android.os.PowerManager
import android.os.SystemClock
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONObject
import org.junit.After
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import java.io.FileInputStream
import java.io.File
import java.util.UUID

/** Real scheduler/service lifecycle tests; no library files or user transfers are changed. */
@RunWith(AndroidJUnit4::class)
class NativeDownloadsTest {
    private val instrumentation get() = InstrumentationRegistry.getInstrumentation()
    private val context get() = instrumentation.targetContext
    private val tokens = mutableListOf<String>()
    private var scenario: ActivityScenario<NativePlayerActivity>? = null
    private var clip: File? = null

    private fun main(action: () -> Unit) = instrumentation.runOnMainSync(action)
    private fun shell(command: String): String = instrumentation.uiAutomation.executeShellCommand(command).use {
        FileInputStream(it.fileDescriptor).bufferedReader().readText()
    }
    private fun await(message: String, condition: () -> Boolean) {
        val deadline = SystemClock.elapsedRealtime() + 15000
        while (SystemClock.elapsedRealtime() < deadline) {
            if (condition()) return
            SystemClock.sleep(100)
        }
        throw AssertionError(message)
    }
    private fun entries(): List<NativeDownloads.Entry> {
        var entries = emptyList<NativeDownloads.Entry>()
        main { entries = NativeDownloads.entries() }
        return entries
    }
    private fun wake() { shell("input keyevent KEYCODE_WAKEUP"); shell("wm dismiss-keyguard") }

    @Before fun prepare() {
        wake()
        if (Build.VERSION.SDK_INT >= 33) {
            instrumentation.uiAutomation.grantRuntimePermission(context.packageName, Manifest.permission.POST_NOTIFICATIONS)
        }
        // A native-only foreground Activity lets this suite test Android's job
        // contract without starting Tauri's process-wide event loop in the runner.
        val fixture = File(context.cacheDir, "download-regression-${UUID.randomUUID()}.mp4")
        clip = fixture
        instrumentation.context.assets.open("native-audio-session-test.mp4").use { source ->
            fixture.outputStream().use { source.copyTo(it) }
        }
        val intent = Intent(context, NativePlayerActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            .putExtra("video", JSONObject().put("videoId", "download-regression")
                .put("video", fixture.name).put("collection", "Download regression fixtures")
                .put("path", fixture.absolutePath).toString())
        scenario = ActivityScenario.launch(intent)
    }

    @After fun cleanup() {
        wake()
        tokens.forEach { NativeDownloads.end(JSONObject().put("id", it).toString()) }
        await("Download jobs were not released") { entries().none { it.id in tokens } }
        main { NativeBridge.playback?.stopPlayback() }
        scenario?.close()
        clip?.delete()
    }

    private fun start(name: String): NativeDownloads.Entry {
        val id = "download-regression-${UUID.randomUUID()}"
        tokens.add(id)
        val result = JSONObject(NativeDownloads.begin(context, JSONObject().put("id", id).put("name", name).toString()))
        assertFalse("Could not start transfer: $result", result.has("error"))
        return entries().first { it.id == id }
    }

    @Test fun screenOffKeepsNativeTransferExecutionAndReleasesItWhenFinished() {
        val entry = start("Screen-off regression fixture")
        if (Build.VERSION.SDK_INT >= 34) {
            val job = context.getSystemService(JobScheduler::class.java).allPendingJobs.first { it.id == entry.notificationId }
            assertTrue(job.isUserInitiated)
            assertNotNull(entry.parameters)
        } else main { assertNotNull(NativeDownloads.legacyService) }
        shell("input keyevent KEYCODE_HOME")
        shell("input keyevent KEYCODE_SLEEP")
        await("Screen stayed on") { !context.getSystemService(PowerManager::class.java).isInteractive }
        val locks = shell("dumpsys power").substringAfter("Wake Locks: size=").substringBefore("Suspend Blockers: size=")
        assertTrue("Transfer has no CPU wake lock while the screen is off: $locks",
            locks.contains("NativeDownloadJobService") || locks.contains("${context.packageName}:downloads"))
        assertTrue(entries().any { it.id == entry.id })
        NativeDownloads.end(JSONObject().put("id", entry.id).toString())
        await("Finished transfer kept its Android execution slot") { entries().none { it.id == entry.id } }
        await("Finished transfer notification remained") {
            context.getSystemService(NotificationManager::class.java).activeNotifications.none { it.id == entry.notificationId }
        }
    }

    @Test fun cancellingOneNotificationPreservesTheOtherTransferAndUpdatesProgress() {
        val first = start("First download fixture")
        val second = start("Second download fixture")
        NativeDownloads.update(JSONObject().put("id", second.id).put("downloaded", 2048).put("total", 4096).toString())
        await("Progress did not reach the native owner") { entries().firstOrNull { it.id == second.id }?.downloaded == 2048L }
        val notifications = context.getSystemService(NotificationManager::class.java)
        if (Build.VERSION.SDK_INT >= 34) {
            await("Download notification is absent") { notifications.activeNotifications.any { it.id == first.notificationId } }
            notifications.activeNotifications.first { it.id == first.notificationId }.notification.actions.last().actionIntent.send()
            await("Notification cancellation did not release its job") { entries().none { it.id == first.id } }
            assertTrue(entries().any { it.id == second.id && it.parameters != null })
        } else {
            NativeDownloads.end(JSONObject().put("id", first.id).toString())
            await("First download did not finish") { entries().none { it.id == first.id } }
            main { assertNotNull(NativeDownloads.legacyService) }
        }
        NativeDownloads.end(JSONObject().put("id", first.id).toString()) // repeated completion is harmless
        NativeDownloads.end(JSONObject().put("id", second.id).toString())
        await("Last download did not release the native owner") { entries().none { it.id in tokens } }
    }

    @Test fun systemStoppingAJobReleasesItsTransferInsteadOfLeavingAnUnprotectedWorker() {
        val entry = start("System-stop regression fixture")
        if (Build.VERSION.SDK_INT >= 34) {
            context.getSystemService(JobScheduler::class.java).cancel(entry.notificationId)
        } else {
            main { NativeDownloads.legacyService?.stopSelf() }
        }
        await("System-stopped transfer remained registered") { entries().none { it.id == entry.id } }
        assertNotNull("System stop did not give the transfer a terminal reason", entry.failure)
    }
}
