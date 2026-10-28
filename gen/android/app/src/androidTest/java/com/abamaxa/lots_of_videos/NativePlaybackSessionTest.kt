package com.abamaxa.lots_of_videos

import android.Manifest
import android.app.Notification
import android.app.NotificationManager
import android.content.Intent
import android.graphics.Color
import android.media.AudioAttributes
import android.media.AudioFocusRequest
import android.media.AudioManager
import android.media.session.MediaController
import android.os.Build
import android.os.PowerManager
import android.os.SystemClock
import android.provider.Settings
import android.view.View
import android.view.ViewGroup
import androidx.lifecycle.Lifecycle
import androidx.test.core.app.ActivityScenario
import androidx.test.espresso.Espresso.onView
import androidx.test.espresso.action.ViewActions.click
import androidx.test.espresso.matcher.ViewMatchers.withContentDescription
import androidx.test.espresso.matcher.ViewMatchers.withText
import androidx.test.espresso.matcher.RootMatchers.isDialog
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.filters.SdkSuppress
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONObject
import org.json.JSONArray
import org.junit.After
import org.junit.Assert.*
import org.junit.Before
import org.junit.Assume.assumeTrue
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.io.FileInputStream

/** Real decoder/session tests; only private disposable fixtures are created, never library files. */
@RunWith(AndroidJUnit4::class)
@SdkSuppress(minSdkVersion = 29)
class NativePlaybackSessionTest {
    private val instrumentation get() = InstrumentationRegistry.getInstrumentation()
    private val context get() = instrumentation.targetContext
    private val clips = mutableListOf<File>()
    private var previousTutorial: String? = null
    private var previousVolume = 0
    private fun shell(command: String): String = instrumentation.uiAutomation.executeShellCommand(command).use {
        FileInputStream(it.fileDescriptor).bufferedReader().readText()
    }
    private fun main(action: () -> Unit) = instrumentation.runOnMainSync(action)
    private fun state() = JSONObject(NativeBridge.playerState)
    private fun await(message: String, timeoutMillis: Long = 15000, condition: () -> Boolean) {
        val deadline = SystemClock.elapsedRealtime() + timeoutMillis
        while (SystemClock.elapsedRealtime() < deadline) {
            if (condition()) return
            SystemClock.sleep(100)
        }
        throw AssertionError("$message: ${NativeBridge.playerState}")
    }
    private fun awaitState(message: String, timeoutMillis: Long = 15000, condition: (JSONObject) -> Boolean) =
        await(message, timeoutMillis) { condition(state()) }
    private fun wake() { shell("input keyevent KEYCODE_WAKEUP"); shell("wm dismiss-keyguard") }
    private fun setTutorial(value: String?) {
        instrumentation.uiAutomation.adoptShellPermissionIdentity(Manifest.permission.WRITE_SECURE_SETTINGS)
        try { Settings.Secure.putString(context.contentResolver, "immersive_mode_confirmations", value) }
        finally { instrumentation.uiAutomation.dropShellPermissionIdentity() }
    }
    @Before fun prepare() {
        wake()
        previousTutorial = Settings.Secure.getString(context.contentResolver, "immersive_mode_confirmations")
        setTutorial("confirmed")
        val audio = context.getSystemService(AudioManager::class.java)
        previousVolume = audio.getStreamVolume(AudioManager.STREAM_MUSIC)
        audio.setStreamVolume(AudioManager.STREAM_MUSIC, audio.getStreamMaxVolume(AudioManager.STREAM_MUSIC) / 2, 0)
        instrumentation.uiAutomation.grantRuntimePermission(context.packageName, Manifest.permission.RECORD_AUDIO)
    }
    @After fun cleanup() {
        wake()
        main { NativeBridge.playback?.stopPlayback() }
        clips.forEach { it.delete() }
        context.getSystemService(AudioManager::class.java).setStreamVolume(AudioManager.STREAM_MUSIC, previousVolume, 0)
        setTutorial(previousTutorial)
    }
    private fun open(asset: String = "native-audio-session-test.mp4", startTime: Double = 0.0, audioOnly: Boolean = false): ActivityScenario<NativePlayerActivity> {
        val clip = File(context.cacheDir, "session-test-$asset")
        clips.add(clip)
        instrumentation.context.assets.open(asset).use { input -> clip.outputStream().use { input.copyTo(it) } }
        val intent = Intent(context, NativePlayerActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            .putExtra("video", JSONObject().put("videoId", "session-test-$asset")
                .put("video", clip.name).put("collection", "Native regression fixtures")
                .put("path", clip.absolutePath).put("startTime", startTime).put("audioOnly", audioOnly).toString())
        val scenario = ActivityScenario.launch<NativePlayerActivity>(intent)
        awaitState("Decoder did not start", if (startTime > 0) 10000 else 45000) { !it.optBoolean("closed") && it.optBoolean("playing") && it.optInt("decodedAudio") > 0 }
        scenario.onActivity { activity ->
            // The decoder may become ready after controls have auto-hidden.
            // Reveal them as a user would before the test operates a control.
            val decor = activity.window.decorView
            if (findControl(decor, "Back")?.isShown != true)
                findControl(decor, "Video playback")?.performClick()
            activity.onUserInteraction()
        }
        return scenario
    }
    private fun notification(): Notification = context.getSystemService(NotificationManager::class.java)
        .activeNotifications.first { it.id == NativePlaybackService.NOTIFICATION }.notification
    private fun controller(): MediaController {
        @Suppress("DEPRECATION")
        val token = notification().extras.getParcelable<android.media.session.MediaSession.Token>(Notification.EXTRA_MEDIA_SESSION)!!
        return MediaController(context, token)
    }
    @Test fun completedSavedPositionStartsAtBeginning() {
        var duration = 0.0
        open("native-av-sync-test.mp4").use {
            awaitState("Duration unavailable") { it.optDouble("duration") > 0 }
            duration = state().getDouble("duration")
            main { NativeBridge.playback!!.seekTo((duration * 1000).toLong() - 800) }
            awaitState("Video did not finish") { it.optBoolean("closed") }
        }
        open("native-av-sync-test.mp4", startTime = duration).use {
            assertFalse("Replayed video immediately closed", state().optBoolean("closed"))
            assertTrue("Replayed video started at the saved end", state().getDouble("currentTime") < duration / 2)
        }
    }
    @Test fun videoAndAudioStayTogetherAfterSeekingAndDisplayChanges() = verifyAudioVideoSync("native-av-sync-test.mp4")
    @Test fun downloadedAv1VideoAndAudioStayTogetherAfterSeekingAndDisplayChanges() = verifyAudioVideoSync("native-av1-sync-test.mp4")
    @Test fun syncProbeRejectsDelayedAudio() {
        val preferences = context.getSharedPreferences("native-playback", android.content.Context.MODE_PRIVATE)
        val previous = preferences.getLong("audio-delay-ms", 0L)
        try {
            try {
                verifyAudioVideoSync("native-av-sync-test.mp4", 1000L)
                fail("Sync probe accepted audio delayed by a full second")
            } catch (error: AssertionError) {
                assertTrue("Probe failed for another reason: ${error.message}",
                    error.message?.contains("Audio/video disagree") == true)
            }
        } finally {
            preferences.edit().putLong("audio-delay-ms", previous).commit()
            main { NativeBridge.playback?.setAudioDelay(previous) }
        }
    }
    @Test fun audioSyncAdvancesSoundAndIsRememberedAcrossPlaybackSessions() {
        val preferences = context.getSharedPreferences("native-playback", android.content.Context.MODE_PRIVATE)
        val previous = preferences.getLong("audio-delay-ms", 0L)
        fun showSync(scenario: ActivityScenario<NativePlayerActivity>) {
            main { NativeBridge.playback!!.pause() }
            awaitState("Playback did not pause for settings") { it.optBoolean("paused") }
            scenario.onActivity { it.onUserInteraction() }
            onView(withContentDescription("Playback settings")).perform(click())
            onView(withText("Audio sync")).inRoot(isDialog()).perform(click())
        }
        try {
            open("native-av-sync-test.mp4").use { scenario ->
                main { assertTrue(NativeBridge.playback!!.setAudioDelay(0)) }
                showSync(scenario)
                onView(withContentDescription("Sound earlier by 50 milliseconds")).inRoot(isDialog()).perform(click())
                awaitState("Earlier did not advance the actual audio output") { it.optLong("audioDelayMs") == -50L }
                onView(withText("Done")).inRoot(isDialog()).perform(click())
                main {
                    NativeBridge.playback!!.apply {
                        pause()
                        seekTo(7000)
                        setDisplayMode("visualizer")
                        setDisplayMode("video")
                    }
                }
                assertEquals(-50L, state().getLong("audioDelayMs"))
                main { NativeBridge.playback!!.stopPlayback() }
            }
            open("native-av-sync-test.mp4").use { scenario ->
                awaitState("Saved audio correction was not restored") { it.optLong("audioDelayMs") == -50L }
                showSync(scenario)
                onView(withText("Reset")).inRoot(isDialog()).perform(click())
                awaitState("Reset did not restore default audio timing") { it.optLong("audioDelayMs") == 0L }
            }
        } finally {
            preferences.edit().putLong("audio-delay-ms", previous).commit()
            main { NativeBridge.playback?.setAudioDelay(previous) }
        }
    }
    private fun verifyAudioVideoSync(asset: String, forcedDelay: Long? = null) {
        // Green frames contain an 880 Hz tone; red frames are silent. Both
        // streams change at the same timestamps, with long GOPs and B frames
        // like downloaded MP4s. Compare the actual picture and PCM output.
        open(asset).use {
            assertEquals("Wrong decoder policy for $asset",
                if (asset.contains("av1")) "software" else "auto", state().getString("videoDecoder"))
            if (forcedDelay != null) main { assertTrue(NativeBridge.playback!!.setAudioDelay(forcedDelay)) }
            NativeAudioAnalysis(state().getInt("audioSessionId")).use { analysis ->
                // Attaching the output observer is asynchronous. Initial empty
                // capture buffers are not evidence that decoded audio is late.
                await("Audio analysis did not observe the playing tone", 10000) {
                    analysis.frame() != null && analysis.rms > 0.005
                }
                fun checkSync() {
                    var compared = 0
                    var mismatches = 0
                    val readings = mutableListOf<String>()
                    val deadline = SystemClock.elapsedRealtime() + 6500
                    while (SystemClock.elapsedRealtime() < deadline) {
                        var position = 0.0
                        main { position = NativeBridge.playback!!.snapshot().getDouble("currentTime") }
                        // Allow the codec/output buffers to settle after a seek
                        // and avoid comparing across a tone/color boundary.
                        val phase = position % 2.0
                        if (phase in 0.4..1.6) {
                            analysis.frame()
                            val before = analysis.rms
                            val captureStart = SystemClock.elapsedRealtime()
                            val image = instrumentation.uiAutomation.takeScreenshot()
                                ?: throw AssertionError("Playback screenshot unavailable")
                            val pixel = image.getPixel(image.width / 2, image.height / 3)
                            image.recycle()
                            analysis.frame()
                            val captureTime = SystemClock.elapsedRealtime() - captureStart
                            var afterPosition = 0.0
                            main { afterPosition = NativeBridge.playback!!.snapshot().getDouble("currentTime") }
                            val green = Color.green(pixel) > Color.red(pixel) * 2
                            val red = Color.red(pixel) > Color.green(pixel) * 2
                            // Screenshot capture and PCM sampling are asynchronous.
                            // Compare only stable audio away from a timestamp edge;
                            // otherwise a healthy transition looks like a sync error.
                            val stable = (before > 0.005) == (analysis.rms > 0.005) &&
                                afterPosition % 2.0 in 0.4..1.6 && captureTime < 300
                            if ((green || red) && stable) {
                                compared++
                                readings.add("t=$position green=$green rms=$before/${analysis.rms} capture=${captureTime}ms")
                                if (green != (analysis.rms > 0.005)) mismatches++
                            }
                        }
                        SystemClock.sleep(60)
                    }
                    assertTrue("No decoded picture/audio samples were compared", compared >= 4)
                    assertTrue("Audio/video disagree in $mismatches of $compared samples: $readings ${state()}",
                        mismatches <= 1)
                }
                checkSync()
                if (forcedDelay != null) return
                main { NativeBridge.playback!!.seekTo(8500) }
                SystemClock.sleep(700)
                checkSync()
                main {
                    NativeBridge.playback!!.apply {
                        pause()
                        seekTo(16500)
                        setDisplayMode("visualizer")
                        setDisplayMode("video")
                        play()
                    }
                }
                SystemClock.sleep(700)
                checkSync()
            }
        }
    }
    @Test fun defaultScreenOffPausesAndDoesNotResumeOnUnlock() {
        open().use { scenario ->
            assertFalse(state().getBoolean("backgroundAudio"))
            assertEquals("video", state().getString("displayMode"))
            val session = state().getString("sessionId")
            shell("input keyevent KEYCODE_SLEEP")
            await("Display stayed awake") { !context.getSystemService(PowerManager::class.java).isInteractive }
            awaitState("Default screen-off playback did not pause") { it.optBoolean("paused") }
            val position = state().getDouble("currentTime")
            SystemClock.sleep(1800)
            main { assertEquals(position, NativeBridge.playback!!.snapshot().getDouble("currentTime"), 0.15) }
            wake()
            scenario.moveToState(Lifecycle.State.RESUMED)
            assertEquals(session, state().getString("sessionId"))
            assertTrue(state().getBoolean("paused"))
            await("Paused video picture did not reconnect after unlock") { videoPictureVisible() }
        }
    }
    @Test fun watchStopsOnBackAndListenReopensWithoutRestarting() {
        open().use {
            assertEquals("watch", state().getString("playbackMode"))
            awaitState("Watch playback did not advance") { it.optDouble("currentTime") > 1 }
            val position = state().getDouble("currentTime")
            onView(withContentDescription("Back")).perform(click())
            awaitState("Watch mode kept playing after Back") { it.optBoolean("closed") }
            assertTrue("Watch mode lost its saved position: ${state()}", state().getDouble("currentTime") >= position - 0.25)
        }
        open(audioOnly = true).use {
            assertEquals("listen", state().getString("playbackMode"))
            val session = state().getString("sessionId")
            val position = state().getDouble("currentTime")
            onView(withContentDescription("Back")).perform(click())
            awaitState("Listen mode stopped after Back") {
                !it.optBoolean("uiVisible") && !it.optBoolean("closed") &&
                    it.optBoolean("playing") && it.optDouble("currentTime") > position + 1
            }
            ActivityScenario.launch<NativePlayerActivity>(Intent(context, NativePlayerActivity::class.java)
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)).use {
                awaitState("Player did not reconnect") { it.optBoolean("uiVisible") && it.optBoolean("playing") }
                assertEquals(session, state().getString("sessionId"))
                assertTrue(state().getDouble("currentTime") > position)
            }
        }
    }
    @Test fun playlistRemembersBothModesAndMixedMediaKeepsTheSessionChoice() {
        val playlistId = "mode-regression-${java.util.UUID.randomUUID()}"
        val preferences = context.getSharedPreferences("native-playback", android.content.Context.MODE_PRIVATE)
        try {
            open().use {
                val path = state().getString("currentSrc")
                val items = JSONArray()
                    .put(JSONObject().put("videoId", "music-video").put("path", path).put("audioOnly", false))
                    .put(JSONObject().put("videoId", "audio-track").put("path", path).put("audioOnly", true))
                val playlist = JSONObject().put("playlistId", playlistId).put("items", items)
                main { NativeBridge.playback!!.open(playlist) }
                awaitState("Playlist did not start") { it.optString("videoId") == "music-video" && it.optBoolean("playing") }
                assertEquals("watch", state().getString("playbackMode"))
                main { NativeBridge.playback!!.setPlaybackMode("listen") }
                main { NativeBridge.playback!!.control(JSONObject().put("action", "next")) }
                awaitState("Mixed-media next did not start") { it.optString("videoId") == "audio-track" && it.optBoolean("playing") }
                assertEquals("listen", state().getString("playbackMode"))
                main { NativeBridge.playback!!.open(playlist) }
                awaitState("Remembered playlist did not restart") { it.optString("videoId") == "music-video" && it.optBoolean("playing") }
                assertEquals("listen", state().getString("playbackMode"))
                main { NativeBridge.playback!!.setPlaybackMode("watch") }
                main { NativeBridge.playback!!.open(playlist.put("startVideoId", "audio-track")) }
                awaitState("Audio selection did not start") { it.optString("videoId") == "audio-track" && it.optBoolean("playing") }
                assertEquals("watch", state().getString("playbackMode"))
                assertEquals("watch", preferences.getString("playlist-mode:$playlistId", null))
            }
        } finally { preferences.edit().remove("playlist-mode:$playlistId").commit() }
    }
    @Test fun addingToTheQueueWhileBrowsingPreservesPlaybackAndAdvancesInListenMode() {
        open(audioOnly = true).use { scenario ->
            val before = state()
            val item = JSONObject().put("videoId", "queued-music-video").put("video", "Music video")
                .put("path", before.getString("currentSrc")).put("audioOnly", false)
            scenario.moveToState(Lifecycle.State.CREATED)
            main { NativeBridge.playback!!.control(JSONObject().put("action", "enqueue").put("item", item)) }
            assertEquals(before.getString("sessionId"), state().getString("sessionId"))
            assertEquals(before.getString("videoId"), state().getString("videoId"))
            assertEquals(2, state().getInt("queueLength"))
            assertFalse(state().getBoolean("paused"))
            awaitState("Track duration unavailable") { it.optDouble("duration") > 0 }
            main { NativeBridge.playback!!.seekTo((state().getDouble("duration") * 1000).toLong() - 1500) }
            awaitState("Appended item did not play while browsing", 45000) {
                it.optString("videoId") == "queued-music-video" && it.optBoolean("playing") && it.optInt("decodedAudio") > 0
            }
            assertEquals("listen", state().getString("playbackMode"))
            assertEquals(before.getString("sessionId"), state().getString("sessionId"))
        }
    }
    @Test fun playlistAdvancesWhileLockedAndRetainsSettingsAndMediaSkipControls() {
        open().use { scenario ->
            val path = state().getString("currentSrc")
            val items = JSONArray()
            for (index in 0..2) items.put(JSONObject().put("videoId", "playlist-$index")
                .put("video", "Playlist item $index").put("collection", "Playlist regression fixtures")
                .put("path", path).put("startTime", 0))
            main { NativeBridge.playback!!.open(JSONObject().put("playlistId", "regression-playlist")
                .put("playlistName", "Regression mix").put("items", items)) }
            awaitState("First playlist item did not start") { it.optString("videoId") == "playlist-0" && it.optBoolean("playing") && it.optDouble("duration") > 0 }
            val sessionId = state().getString("sessionId")
            main {
                NativeBridge.playback!!.apply {
                    setPlaybackMode("listen")
                    setDisplayMode("visualizer")
                    control(JSONObject().put("action", "rate").put("rate", 1.25))
                    control(JSONObject().put("action", "volume").put("volume", 67))
                    seekTo((snapshot().getDouble("duration") * 1000).toLong() - 2500)
                }
            }
            shell("input keyevent KEYCODE_SLEEP")
            awaitState("Playlist did not advance with the screen locked", 30000) {
                it.optString("videoId") == "playlist-1" && it.optBoolean("playing") && it.optInt("decodedAudio") > 0
            }
            assertEquals(sessionId, state().getString("sessionId"))
            assertTrue(state().getBoolean("backgroundAudio"))
            assertEquals("visualizer", state().getString("displayMode"))
            assertEquals(1.25, state().getDouble("playbackRate"), 0.01)
            assertEquals(67, state().getInt("volume"))
            assertEquals(1, state().getInt("queueIndex"))
            assertEquals(3, state().getInt("queueLength"))
            val controller = controller()
            controller.transportControls.skipToPrevious()
            awaitState("Media previous did not select the previous item") { it.optString("videoId") == "playlist-0" && it.optBoolean("playing") }
            controller.transportControls.skipToNext()
            awaitState("Media next did not select the next item") { it.optString("videoId") == "playlist-1" && it.optBoolean("playing") }
            wake()
            scenario.moveToState(Lifecycle.State.RESUMED)
            // Keep controls visible while Android finishes its unlock/orientation
            // animation; otherwise Espresso can outwait the three-second timeout.
            main { NativeBridge.playback!!.pause() }
            awaitState("Pause after unlock did not settle") { it.optBoolean("paused") }
            scenario.onActivity { activity ->
                findControl(activity.window.decorView, "Video playback")?.performClick()
                activity.onUserInteraction()
            }
            onView(withContentDescription("Next item")).perform(click())
            awaitState("Player next did not select the last item") { it.optString("videoId") == "playlist-2" && it.optBoolean("playing") && it.optDouble("duration") > 0 }
            main { NativeBridge.playback!!.seekTo((state().getDouble("duration") * 1000).toLong() - 1500) }
            awaitState("Playback did not stop at the end of the playlist", 20000) { it.optBoolean("closed") }
        }
    }
    @Test fun playlistSkipsFilesRemovedAfterOpeningAndKeepsItsPositionAtTheBoundary() {
        open().use {
            val path = state().getString("currentSrc")
            val temporary = File(context.cacheDir, "playlist-disposable-next.mp4")
            clips.add(temporary)
            File(path).copyTo(temporary, overwrite = true)
            val items = JSONArray()
            for (index in 0..2) items.put(JSONObject().put("videoId", "skip-$index")
                .put("video", "Skip test $index").put("path", if (index == 1) temporary.absolutePath else path))
            main { NativeBridge.playback!!.open(JSONObject().put("playlistId", "skip-regression").put("items", items)) }
            awaitState("Skip playlist did not start") { it.optString("videoId") == "skip-0" && it.optBoolean("playing") }
            temporary.delete()
            main { NativeBridge.playback!!.control(JSONObject().put("action", "next")) }
            awaitState("Removed item was not skipped") { it.optString("videoId") == "skip-2" && it.optBoolean("playing") }
            main { NativeBridge.playback!!.control(JSONObject().put("action", "previous")) }
            awaitState("Previous did not skip removed item") { it.optString("videoId") == "skip-0" && it.optBoolean("playing") }
            main { NativeBridge.playback!!.control(JSONObject().put("action", "previous")) }
            assertEquals(0, state().getInt("queueIndex"))
            assertEquals("skip-0", state().getString("videoId"))
        }
    }
    @Test fun pausedDisplaySwitchPreservesPositionAndResumesFromTheSeekTarget() {
        open().use {
            val position = 17000L
            main {
                val playback = NativeBridge.playback!!
                playback.pause()
                playback.control(JSONObject().put("action", "rate").put("rate", 1.5))
                playback.control(JSONObject().put("action", "audio").put("track", playback.tracks(false).last { it.id >= 0 }.id))
                playback.seekTo(position)
            }
            awaitState("Paused seek was not reported") { kotlin.math.abs(it.optDouble("currentTime") - position / 1000.0) < 0.2 }
            for (mode in listOf("visualizer", "video")) {
                main {
                    NativeBridge.playback!!.setDisplayMode(mode)
                }
                SystemClock.sleep(1500)
                main {
                    val current = NativeBridge.playback!!.snapshot()
                    assertTrue("Resumed while selecting $mode", current.getBoolean("paused"))
                    assertFalse("Decoder played while selecting $mode: $current", current.getBoolean("playing"))
                    assertEquals("Paused seek moved while selecting $mode", position / 1000.0, current.getDouble("currentTime"), 0.15)
                }
            }
            var resumedAt = 0L
            main {
                resumedAt = SystemClock.elapsedRealtime()
                NativeBridge.playback!!.play()
            }
            awaitResumedPosition(position, resumedAt)
        }
    }
    private fun awaitResumedPosition(position: Long, resumedAt: Long, timeoutMillis: Long = 15000) {
        val deadline = SystemClock.elapsedRealtime() + timeoutMillis
        var previousPosition: Double? = null
        var current = JSONObject()
        var elapsed = 0.0
        while (SystemClock.elapsedRealtime() < deadline) {
            main {
                current = NativeBridge.playback!!.snapshot()
                elapsed = (SystemClock.elapsedRealtime() - resumedAt) / 1000.0
            }
            val time = current.optDouble("currentTime")
            val withinBound = current.optBoolean("playing") && time > position / 1000.0 + 0.25 &&
                time <= position / 1000.0 + elapsed * 1.5 + 0.4
            // A Playing notification can precede completion of the precise seek.
            // Keep the original error bound and require two advancing observations.
            if (withinBound) {
                val previous = previousPosition
                if (previous != null && time > previous + 0.25) return
                if (previous == null) previousPosition = time
            } else previousPosition = null
            SystemClock.sleep(100)
        }
        throw AssertionError("Playback did not settle at saved position $position: elapsed=$elapsed $current")
    }
    @Test fun resumePositionCheckRejectsWrongPosition() {
        open().use {
            main {
                NativeBridge.playback!!.apply {
                    pause()
                    control(JSONObject().put("action", "rate").put("rate", 1.5))
                    seekTo(22000)
                }
            }
            main { NativeBridge.playback!!.play() }
            awaitState("Negative control did not resume from the wrong position") {
                it.optBoolean("playing") && it.optDouble("currentTime") > 22.25
            }
            val checkedAt = SystemClock.elapsedRealtime()
            try {
                awaitResumedPosition(17000, checkedAt, 1500)
                fail("Resume position check accepted a five-second position error")
            } catch (error: AssertionError) {
                assertTrue("Position check failed for another reason: ${error.message}",
                    error.message?.contains("Playback did not settle at saved position") == true)
            }
        }
    }
    @Test fun displaySwitchPreservesStateAndReadsRealAudio() {
        open().use { scenario ->
            var selectedAudioTrack = -1
            main {
                val playback = NativeBridge.playback!!
                playback.pause()
                playback.control(JSONObject().put("action", "rate").put("rate", 1.5))
                playback.control(JSONObject().put("action", "volume").put("volume", 67))
                val tracks = playback.tracks(false).filter { it.id >= 0 }
                assertEquals(2, tracks.size)
                selectedAudioTrack = tracks.last().id
                playback.control(JSONObject().put("action", "audio").put("track", selectedAudioTrack))
                playback.seekTo(17000)
            }
            // VLC applies track, rate and volume changes asynchronously. Capture
            // the preservation baseline from its actual settled state, not the
            // optimistic seek notification published before those changes finish.
            var before = JSONObject()
            await("Paused seek and playback settings did not settle") {
                main { before = NativeBridge.playback!!.snapshot() }
                before.optDouble("currentTime") in 16.8..17.2 && before.optInt("volume") == 67 &&
                    kotlin.math.abs(before.optDouble("playbackRate") - 1.5) < 0.01 &&
                    before.optInt("currentAudioTrack") == selectedAudioTrack &&
                    before.optBoolean("paused") && !before.optBoolean("playing")
            }
            onView(withText("Visualizer")).perform(click())
            awaitState("Visualizer was not selected") { it.optString("displayMode") == "visualizer" }
            assertFalse(state().getBoolean("backgroundAudio"))
            SystemClock.sleep(500)
            for (key in listOf("sessionId", "currentAudioTrack", "volume", "playbackRate", "paused"))
                assertEquals("Changed $key", before.get(key), state().get(key))
            assertEquals(before.getDouble("currentTime"), state().getDouble("currentTime"), 0.15)
            onView(withText("Video")).perform(click())
            assertEquals("video", state().getString("displayMode"))
            assertTrue(state().getBoolean("paused"))
            for (key in listOf("sessionId", "currentAudioTrack", "volume", "playbackRate"))
                assertEquals("Changed $key returning to Video", before.get(key), state().get(key))
            assertEquals(before.getDouble("currentTime"), state().getDouble("currentTime"), 0.15)
            await("Paused video picture did not return") {
                // The fixture has a green video frame. This also catches a black,
                // detached surface even when the playback state still looks correct.
                videoPictureVisible()
            }
            main {
                val playback = NativeBridge.playback!!
                playback.control(JSONObject().put("action", "audio").put("track", playback.tracks(false).first { it.id >= 0 }.id))
                playback.seekTo(1000)
                playback.play()
            }
            onView(withText("Visualizer")).perform(click())
            var frames = 0
            var rms = 0.0
            await("Visualizer did not receive the playing tone") {
                scenario.onActivity { activity ->
                    val view = findVisualizer(activity.window.decorView)
                    frames = view?.analysisFrames ?: 0
                    rms = view?.analysisRms ?: 0.0
                }
                frames > 3 && rms > 0.005
            }
            main { NativeBridge.playback!!.seekTo(13000) }
            awaitState("Silence seek did not settle") { it.optDouble("currentTime") > 12.8 }
            SystemClock.sleep(800)
            await("Silent section did not produce quiet analysis") {
                scenario.onActivity { rms = findVisualizer(it.window.decorView)?.analysisRms ?: 1.0 }
                rms < 0.002
            }
            main { NativeBridge.playback!!.pause() }
            awaitState("Pause did not settle") { it.optBoolean("paused") }
            assertEquals(before.getString("sessionId"), state().getString("sessionId"))
        }
    }
    @Test fun backgroundAudioSurvivesPowerButtonAndActivityDestructionWithMediaControls() {
        var original: NativePlaybackService? = null
        val scenario = open()
        try {
            onView(withContentDescription("Listen mode")).perform(click())
            val session = state().getString("sessionId")
            main { original = NativeBridge.playback }
            val start = state().getDouble("currentTime")
            val remote = controller()
            assertTrue(remote.metadata!!.getString(android.media.MediaMetadata.METADATA_KEY_TITLE).contains("session-test"))
            shell("input keyevent KEYCODE_SLEEP")
            await("Display stayed awake") { !context.getSystemService(PowerManager::class.java).isInteractive }
            awaitState("Screen-off audio did not advance") { it.optDouble("currentTime") > start + 2.5 && !it.optBoolean("paused") }
            remote.transportControls.pause()
            awaitState("Lock-screen pause failed") { it.optBoolean("paused") }
            val paused = state().getDouble("currentTime")
            SystemClock.sleep(1200)
            assertEquals(paused, state().getDouble("currentTime"), 0.15)
            notification().actions[1].actionIntent.send()
            awaitState("Notification play failed") { !it.optBoolean("paused") }
            remote.transportControls.seekTo(35000)
            awaitState("Lock-screen seek failed") { it.optDouble("currentTime") > 34.8 }
            notification().actions[2].actionIntent.send()
            awaitState("Notification forward failed") { it.optDouble("currentTime") > 64.5 }
            wake()
            scenario.moveToState(Lifecycle.State.RESUMED)
            scenario.close()
            assertFalse(state().getBoolean("closed"))
            ActivityScenario.launch<NativePlayerActivity>(Intent(context, NativePlayerActivity::class.java)
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)).use {
                // A playing fast seek can still settle while the window reconnects.
                // Require the same session to actually advance beyond the seek target.
                awaitState("Reconnected session did not advance past the seek target") {
                    it.optBoolean("playing") && it.optDouble("currentTime") > 64.5
                }
                main { assertSame(original, NativeBridge.playback) }
                assertEquals(session, state().getString("sessionId"))
                assertTrue(state().getDouble("currentTime") > 64.5)
                assertEquals("video", state().getString("displayMode"))
                remote.transportControls.stop()
                awaitState("Media stop failed") { it.optBoolean("closed") }
            }
            await("Stopped playback service was retained") { NativeBridge.playback == null }
            assertTrue(context.getSystemService(NotificationManager::class.java).activeNotifications.none { it.id == NativePlaybackService.NOTIFICATION })
        } finally { scenario.close() }
    }
    @Test fun interruptionsResumeOnlyWhenAppropriate() {
        open().use { scenario ->
            onView(withContentDescription("Listen mode")).perform(click())
            scenario.moveToState(Lifecycle.State.CREATED)
            val audio = context.getSystemService(AudioManager::class.java)
            val interruption = AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN_TRANSIENT)
                .setAudioAttributes(AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_ASSISTANCE_NAVIGATION_GUIDANCE).build())
                .setOnAudioFocusChangeListener { }.build()
            try {
                main { assertEquals(AudioManager.AUDIOFOCUS_REQUEST_GRANTED, audio.requestAudioFocus(interruption)) }
                awaitState("Transient interruption did not pause") { it.optBoolean("paused") }
                main { audio.abandonAudioFocusRequest(interruption) }
                awaitState("Focus return did not resume") { !it.optBoolean("paused") }
                main { audio.requestAudioFocus(interruption) }
                awaitState("Second interruption did not pause") { it.optBoolean("paused") }
                main { NativeBridge.playback!!.pause(); audio.abandonAudioFocusRequest(interruption) }
                SystemClock.sleep(1000)
                assertTrue("A user pause must cancel automatic resume", state().getBoolean("paused"))
                main { NativeBridge.playback!!.play() }
                awaitState("Playback did not resume") { !it.optBoolean("paused") }
            } finally {
                main { audio.abandonAudioFocusRequest(interruption) }
                scenario.moveToState(Lifecycle.State.RESUMED)
            }
        }
    }
    @Test fun headsetRemovalPausesBackgroundPlayback() {
        // Android protects this broadcast from ordinary apps and ADB shells.
        // A userdebug emulator can send it as the system UID; phones need a real unplug check.
        assumeTrue("Protected broadcast simulation needs a userdebug emulator", Build.TYPE == "userdebug" &&
            Build.HARDWARE in listOf("ranchu", "goldfish"))
        open().use { scenario ->
            onView(withContentDescription("Listen mode")).perform(click())
            scenario.moveToState(Lifecycle.State.CREATED)
            val result = shell("su 1000 am broadcast -a android.media.AUDIO_BECOMING_NOISY -p ${context.packageName}")
            assertTrue(result, result.contains("Broadcast completed"))
            awaitState("Headset removal did not pause") { it.optBoolean("paused") }
            scenario.moveToState(Lifecycle.State.RESUMED)
        }
    }
    @Test fun mkvAndEac3StillDecodeInTheDurableSession() {
        open("native-eac3-test.mkv").use {
            assertTrue(state().getInt("decodedAudio") > 0)
            awaitState("MKV video did not decode") { it.optInt("decodedVideo") > 0 }
            main { NativeBridge.playback!!.seekTo(6000) }
            awaitState("MKV seeking failed") { it.optDouble("currentTime") > 5.8 }
        }
    }
    private fun videoPictureVisible(): Boolean {
        val image = instrumentation.uiAutomation.takeScreenshot()
        val pixel = image.getPixel(image.width / 2, image.height / 3)
        image.recycle()
        return Color.green(pixel) > 20
    }

    private fun findVisualizer(view: View): NativeVisualizerView? {
        if (view is NativeVisualizerView) return view
        if (view is ViewGroup) for (i in 0 until view.childCount) findVisualizer(view.getChildAt(i))?.let { return it }
        return null
    }

    private fun findControl(view: View, description: String): View? {
        if (view.contentDescription == description) return view
        if (view is ViewGroup) for (i in 0 until view.childCount)
            findControl(view.getChildAt(i), description)?.let { return it }
        return null
    }
}
