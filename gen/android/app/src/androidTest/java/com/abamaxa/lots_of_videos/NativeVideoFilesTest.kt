package com.abamaxa.lots_of_videos

import android.content.Context
import android.content.Intent
import android.net.Uri
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONArray
import org.json.JSONObject
import org.junit.After
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File

@RunWith(AndroidJUnit4::class)
class NativeVideoFilesTest {
    private lateinit var context: Context
    private lateinit var directory: File
    private lateinit var video: File

    @Before fun setUp() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        context = instrumentation.targetContext
        directory = File(context.filesDir.parentFile, "movies/.native-file-actions-test").apply { mkdirs() }
        video = File(directory, "Clip #1 & café.mp4")
        instrumentation.context.assets.open("native-player-ui-test.mp4").use { input ->
            video.outputStream().use { input.copyTo(it) }
        }
    }

    @After fun cleanUp() { directory.deleteRecursively() }

    private fun request(file: File = video) = JSONObject().put("path", file.absolutePath)

    @Test fun sharesReadableContentUriWithReadOnlyPermission() {
        val intent = NativeVideoFiles.shareIntent(context, JSONObject().put("files", JSONArray().put(request())))
        assertEquals(Intent.ACTION_SEND, intent.action)
        assertEquals("video/mp4", intent.type)
        val uri = intent.clipData!!.getItemAt(0).uri
        assertEquals("content", uri.scheme)
        assertEquals(Intent.FLAG_GRANT_READ_URI_PERMISSION, intent.flags)
        assertArrayEquals(video.readBytes(), context.contentResolver.openInputStream(uri)!!.use { it.readBytes() })
    }

    @Test fun collectionShareIncludesEveryFile() {
        val other = File(directory, "Second video.mp4").apply { writeBytes(video.readBytes()) }
        val intent = NativeVideoFiles.shareIntent(context, JSONObject().put("files", JSONArray().put(request()).put(request(other))))
        assertEquals(Intent.ACTION_SEND_MULTIPLE, intent.action)
        assertEquals(2, intent.clipData!!.itemCount)
        assertEquals(2, intent.getParcelableArrayListExtra<Uri>(Intent.EXTRA_STREAM)!!.size)
    }

    @Test fun exportCreatesACopyWithOriginalNameAndBytes() {
        val intent = NativeVideoFiles.exportIntent(video)
        assertEquals(Intent.ACTION_CREATE_DOCUMENT, intent.action)
        assertTrue(intent.hasCategory(Intent.CATEGORY_OPENABLE))
        assertEquals(video.name, intent.getStringExtra(Intent.EXTRA_TITLE))
        val copy = File(context.cacheDir, "native-export-test.mp4")
        try {
            NativeVideoFiles.copyTo(context, video, Uri.fromFile(copy))
            assertArrayEquals(video.readBytes(), copy.readBytes())
            assertTrue(video.exists())
        } finally { copy.delete() }
    }

    @Test fun rejectsFilesOutsideTheVideoLibrary() {
        val privateFile = File(context.cacheDir, "not-a-library-video.mp4").apply { writeText("private") }
        try {
            assertThrows(IllegalStateException::class.java) { NativeVideoFiles.libraryFile(context, request(privateFile)) }
        } finally { privateFile.delete() }
    }
}
