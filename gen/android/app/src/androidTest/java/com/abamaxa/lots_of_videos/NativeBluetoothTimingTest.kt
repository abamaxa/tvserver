package com.abamaxa.lots_of_videos

import android.content.Intent
import android.os.SystemClock
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Assume.assumeTrue
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File

/** Opt-in physical Bluetooth check. The listening comparison verifies the sound at the receiver. */
@RunWith(AndroidJUnit4::class)
class NativeBluetoothTimingTest {
    @Test fun reportedOutputDelayIsAutomaticAndIndependentOfTheSavedCorrection() {
        assumeTrue(InstrumentationRegistry.getArguments().getString("bluetooth") == "true")
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val context = instrumentation.targetContext
        val sample = File(context.cacheDir, "sync-listening-sample.mp4")
        check(sample.isFile)
        val preferences = context.getSharedPreferences("native-playback", android.content.Context.MODE_PRIVATE)
        val previous = preferences.getLong("audio-delay-ms", 0)
        preferences.edit().putLong("audio-delay-ms", 0).commit()
        val request = JSONObject().put("videoId", "bluetooth-regression").put("video", "Bluetooth timing check")
            .put("path", sample.absolutePath).put("startTime", 40)
        fun main(action: () -> Unit) = instrumentation.runOnMainSync(action)
        fun state(): JSONObject {
            var result = JSONObject()
            main { result = NativeBridge.playback?.snapshot() ?: JSONObject() }
            return result
        }
        try {
            val intent = Intent(context, NativePlayerActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                .putExtra("video", request.toString())
            ActivityScenario.launch<NativePlayerActivity>(intent).use {
                val deadline = SystemClock.elapsedRealtime() + 15000
                while (state().optLong("outputLatencyMs") < 100 && SystemClock.elapsedRealtime() < deadline) SystemClock.sleep(100)
                val automatic = state()
                println("Bluetooth automatic timing: $automatic")
                assertTrue("No Bluetooth output compensation: $automatic", automatic.optLong("outputLatencyMs") >= 100)
                assertEquals(0L, automatic.getLong("audioDelayMs"))
                assertTrue(kotlin.math.abs(automatic.getLong("effectiveAudioDelayMs") + automatic.getLong("outputLatencyMs")) <= 1)
                main { assertTrue(NativeBridge.playback!!.setAudioDelay(-50)) }
                val adjusted = state()
                assertEquals(-50L, preferences.getLong("audio-delay-ms", 0))
                assertEquals(-50L, adjusted.getLong("audioDelayMs"))
                assertTrue(kotlin.math.abs(adjusted.getLong("effectiveAudioDelayMs") + adjusted.getLong("outputLatencyMs") + 50) <= 1)
                main { assertTrue(NativeBridge.playback!!.setAudioDelay(0)) }
                assertTrue(kotlin.math.abs(state().getLong("effectiveAudioDelayMs") + state().getLong("outputLatencyMs")) <= 1)
            }
        } finally {
            main { NativeBridge.playback?.stopPlayback() }
            preferences.edit().putLong("audio-delay-ms", previous).commit()
        }
    }
}
