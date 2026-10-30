package com.abamaxa.lots_of_videos

import android.content.Context
import android.content.ContextWrapper
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONObject
import org.junit.After
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.util.UUID

@RunWith(AndroidJUnit4::class)
class NativeSettingsTest {
    private val actual = InstrumentationRegistry.getInstrumentation().targetContext
    private val directory = File(actual.cacheDir, "settings-test-${UUID.randomUUID()}").apply { mkdirs() }.canonicalFile
    private val context = object : ContextWrapper(actual) {
        override fun getFilesDir(): File = File(directory, "files").apply { mkdirs() }
        override fun getExternalFilesDirs(type: String?): Array<File> = emptyArray()
    }
    @After fun cleanup() { directory.deleteRecursively() }

    @Test fun persistsDefaultsAndKeepsOtherValuesWhenAudioSyncChanges() {
        NativeSettingsStore.update(context, NativePlaybackDefaults.STORE) {
            it.put("rate", 1.5).put("audioDelayMs", -150).put("videoMode", "listen").put("audioLanguage", "es-ES")
                .put("subtitleLanguage", "eng").put("subtitles", false).put("displayMode", "visualizer")
        }
        val first = NativePlaybackDefaults.load(context)
        assertEquals(1.5f, first.rate)
        assertEquals(-150L, first.audioDelayMs)
        assertEquals(NativePlaybackMode.LISTEN, first.modeFor(JSONObject()))
        assertEquals("spa", NativePlaybackDefaults.vlcLanguage(first.audioLanguage))
        assertFalse(first.subtitles)
        assertEquals("visualizer", first.displayMode)
        NativeSettingsStore.update(context, NativePlaybackDefaults.STORE) { it.put("audioDelayMs", 50) }
        val reloaded = NativePlaybackDefaults.load(context)
        assertEquals(1.5f, reloaded.rate)
        assertEquals(50L, reloaded.audioDelayMs)
    }

    @Test fun validatesSavedValuesAndUsesCompatibleInitialModes() {
        val empty = NativePlaybackDefaults.fromJson(JSONObject(), -75)
        assertEquals(1f, empty.rate)
        assertEquals(-75L, empty.audioDelayMs)
        assertEquals(NativePlaybackMode.WATCH, empty.modeFor(JSONObject()))
        assertEquals(NativePlaybackMode.LISTEN, empty.modeFor(JSONObject().put("audioOnly", true)))
        val invalid = NativePlaybackDefaults.fromJson(JSONObject().put("rate", -5).put("audioDelayMs", 999999)
            .put("videoMode", "invalid").put("audioLanguage", "not a language").put("displayMode", "invalid"))
        assertEquals(1f, invalid.rate)
        assertEquals(5000L, invalid.audioDelayMs)
        assertEquals(NativePlaybackMode.WATCH, invalid.videoMode)
        assertEquals("", invalid.audioLanguage)
        assertEquals("video", invalid.displayMode)
        assertEquals("eng", NativePlaybackDefaults.vlcLanguage("en-US"))
        assertEquals("spa", NativePlaybackDefaults.vlcLanguage("spa"))
        assertNull(NativePlaybackDefaults.vlcLanguage(""))
        assertThrows(IllegalArgumentException::class.java) { NativePlaybackDefaults.vlcLanguage("../secret") }
    }

    @Test fun movingKeepsDatabaseAndSwitchesThePointerOnlyAfterVerification() {
        val original = File(directory, "movies/clip.mp4").apply { parentFile!!.mkdirs(); writeText("video") }
        val database = File(directory, "tvserver.sqlite").apply { writeText("database") }
        val volume = NativeStorage.volumes(context).single()
        val destination = NativeStorage.planMove(context, volume)
        assertEquals(directory, NativeStorage.mediaRoot(context))
        assertFalse(NativeStorage.available(context))
        NativeStorage.move(context, destination) {}
        assertEquals(destination, NativeStorage.mediaRoot(context))
        assertTrue(NativeStorage.available(context))
        assertEquals("video", File(destination, "movies/clip.mp4").readText())
        assertFalse(original.exists())
        assertEquals("database", database.readText())
    }

    @Test fun interruptedCopyRecoversWithoutLosingTheOriginalLibrary() {
        val original = File(directory, "movies/clip.mp4").apply { parentFile!!.mkdirs(); writeText("original") }
        val destination = NativeStorage.planMove(context, NativeStorage.volumes(context).single())
        File(destination, "movies/clip.mp4").apply { parentFile!!.mkdirs(); writeText("partial") }
        NativeStorage.recover(context)
        assertEquals(directory, NativeStorage.mediaRoot(context))
        assertEquals("original", original.readText())
        assertFalse(destination.exists())
        assertTrue(NativeStorage.available(context))
    }

    @Test fun absentCardNeverFallsBackToAnEmptyInternalLibrary() {
        NativeSettingsStore.update(context, "media-storage") { it.put("root", "/storage/missing-card/Android/data/app/files/library") }
        assertFalse(NativeStorage.available(context))
        assertThrows(IllegalStateException::class.java) { NativeStorage.requireMediaRoot(context) }
        assertEquals("/storage/missing-card/Android/data/app/files/library", NativeStorage.mediaRoot(context).path)
    }

    @Test fun maintenanceLockPreventsRecoveryFromRemovingACopyInProgress() {
        val destination = NativeStorage.planMove(context, NativeStorage.volumes(context).single())
        File(destination, "movies/clip.mp4").apply { parentFile!!.mkdirs(); writeText("copying") }
        NativeStorage.withMaintenance(context) {
            assertThrows(IllegalStateException::class.java) { NativeStorage.recover(context) }
            assertTrue(destination.isDirectory)
        }
    }
}
