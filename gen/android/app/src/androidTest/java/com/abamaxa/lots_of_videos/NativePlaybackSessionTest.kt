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
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.filters.SdkSuppress
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONObject
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
    private fun open(asset: String = "native-audio-session-test.mp4"): ActivityScenario<NativePlayerActivity> {
        val clip = File(context.cacheDir, "session-test-$asset")
        clips.add(clip)
        instrumentation.context.assets.open(asset).use { input -> clip.outputStream().use { input.copyTo(it) } }
        val intent = Intent(context, NativePlayerActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            .putExtra("video", JSONObject().put("videoId", "session-test-$asset")
                .put("video", clip.name).put("collection", "Native regression fixtures")
                .put("path", clip.absolutePath).toString())
        val scenario = ActivityScenario.launch<NativePlayerActivity>(intent)
        awaitState("Decoder did not start", 45000) { !it.optBoolean("closed") && it.optBoolean("playing") && it.optInt("decodedAudio") > 0 }
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
    @Test fun displaySwitchPreservesStateAndReadsRealAudio() {
        open().use { scenario ->
            main {
                val playback = NativeBridge.playback!!
                playback.pause()
                playback.control(JSONObject().put("action", "rate").put("rate", 1.5))
                playback.control(JSONObject().put("action", "volume").put("volume", 67))
                val tracks = playback.tracks(false).filter { it.id >= 0 }
                assertEquals(2, tracks.size)
                playback.control(JSONObject().put("action", "audio").put("track", tracks.last().id))
                playback.seekTo(17000)
            }
            awaitState("Seek did not settle") { it.optDouble("currentTime") in 16.8..17.2 }
            val before = state()
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
            onView(withContentDescription("Keep audio playing with screen off")).perform(click())
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
            onView(withContentDescription("Keep audio playing with screen off")).perform(click())
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
            onView(withContentDescription("Keep audio playing with screen off")).perform(click())
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
