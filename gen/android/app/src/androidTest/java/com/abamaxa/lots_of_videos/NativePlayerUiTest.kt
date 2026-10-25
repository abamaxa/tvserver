package com.abamaxa.lots_of_videos

import android.Manifest
import android.content.Intent
import android.content.res.Configuration
import android.os.SystemClock
import android.provider.Settings
import androidx.core.view.WindowInsetsCompat
import androidx.test.core.app.ActivityScenario
import androidx.test.espresso.Espresso.onView
import androidx.test.espresso.action.ViewActions.click
import androidx.test.espresso.assertion.ViewAssertions.matches
import androidx.test.espresso.matcher.ViewMatchers.isDisplayed
import androidx.test.espresso.matcher.ViewMatchers.withContentDescription
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.filters.SdkSuppress
import androidx.test.platform.app.InstrumentationRegistry
import org.hamcrest.Matchers.not
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.Before
import org.junit.After
import org.junit.runner.RunWith
import java.io.File

/** Exercise the real native window, decoder and controls with a private test clip. */
@RunWith(AndroidJUnit4::class)
@SdkSuppress(minSdkVersion = 29)
class NativePlayerUiTest {
    private var previousTutorial: String? = null

    private fun setTutorial(value: String?) {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        instrumentation.uiAutomation.adoptShellPermissionIdentity(Manifest.permission.WRITE_SECURE_SETTINGS)
        try {
            Settings.Secure.putString(instrumentation.targetContext.contentResolver,
                "immersive_mode_confirmations", value)
        } finally {
            instrumentation.uiAutomation.dropShellPermissionIdentity()
        }
    }

    @Before
    fun suppressSystemTutorial() {
        // Test the app's controls without Android's first-use tutorial taking
        // window focus. Restore the device preference after the test.
        previousTutorial = Settings.Secure.getString(
            InstrumentationRegistry.getInstrumentation().targetContext.contentResolver,
            "immersive_mode_confirmations")
        setTutorial("confirmed")
    }

    @After
    fun restoreSystemTutorial() = setTutorial(previousTutorial)

    private fun awaitState(condition: (JSONObject) -> Boolean) {
        val deadline = SystemClock.elapsedRealtime() + 15000
        while (SystemClock.elapsedRealtime() < deadline) {
            if (condition(JSONObject(NativeBridge.playerState))) return
            SystemClock.sleep(100)
        }
        throw AssertionError("Native playback state did not settle: ${NativeBridge.playerState}")
    }

    @Test
    fun playbackIsImmersiveLandscapeAndControlsHideUntilTapped() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val context = instrumentation.targetContext
        val clip = File(context.cacheDir, "native-player-ui-test.mp4")
        instrumentation.context.assets.open("native-player-ui-test.mp4").use { source ->
            clip.outputStream().use { source.copyTo(it) }
        }
        try {
            val intent = Intent(context, NativePlayerActivity::class.java)
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                .putExtra("video", JSONObject().put("videoId", "native-player-ui-test")
                    .put("video", clip.name).put("path", clip.absolutePath).toString())
            ActivityScenario.launch<NativePlayerActivity>(intent).use { scenario ->
                awaitState { it.optString("videoId") == "native-player-ui-test" &&
                    it.optInt("decodedVideo") > 0 && !it.optBoolean("paused", true) }
                scenario.onActivity { activity ->
                    assertEquals(Configuration.ORIENTATION_LANDSCAPE, activity.resources.configuration.orientation)
                    val insets = WindowInsetsCompat.toWindowInsetsCompat(activity.window.decorView.rootWindowInsets)
                    assertFalse(insets.isVisible(WindowInsetsCompat.Type.statusBars()))
                    assertFalse(insets.isVisible(WindowInsetsCompat.Type.navigationBars()))
                }
                SystemClock.sleep(4000)
                onView(withContentDescription("Back")).check(matches(not(isDisplayed())))
                onView(withContentDescription("Video position")).check(matches(not(isDisplayed())))

                onView(withContentDescription("Video playback")).perform(click())
                onView(withContentDescription("Back")).check(matches(isDisplayed()))
                onView(withContentDescription("Pause")).perform(click())
                awaitState { it.optBoolean("paused") }
                SystemClock.sleep(4000)
                onView(withContentDescription("Back")).check(matches(isDisplayed()))

                onView(withContentDescription("Play")).perform(click())
                awaitState { !it.optBoolean("paused", true) }
                SystemClock.sleep(4000)
                onView(withContentDescription("Back")).check(matches(not(isDisplayed())))
                onView(withContentDescription("Video playback")).perform(click())
                onView(withContentDescription("Back")).perform(click())
                awaitState { it.optBoolean("closed") }
                assertTrue(clip.exists())
            }
        } finally {
            clip.delete()
        }
    }
}
