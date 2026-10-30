package com.abamaxa.lots_of_videos

import org.junit.After
import org.junit.Assert.*
import org.junit.Test
import java.io.File
import java.nio.file.Files

class MediaStorageMigrationTest {
    private val root = Files.createTempDirectory("media-migration-").toFile().canonicalFile
    private val source = File(root, "internal").apply { mkdir() }
    private val destination = File(root, "sd-card")
    @After fun cleanup() { root.deleteRecursively() }

    private fun media(path: String, data: String = path): File = File(source, path).apply {
        parentFile!!.mkdirs()
        writeText(data)
    }

    @Test fun copiesAllMediaAndHiddenFilesWithoutMovingPrivateDataOrRemovingTheSource() {
        val paths = listOf("movies/Music/café.mp3", "movies/.downloads/unfinished.part", "movies/.thumbnails/1.jpg",
            "books/Stories/book.epub", "books/.thumbnails/cover.jpg", "movies/Empty/.keep")
        paths.forEach { media(it) }
        media("tvserver.sqlite", "private database")
        media("config/settings", "private preferences")
        MediaStorageMigration.copy(source, destination)
        paths.forEach { path ->
            assertArrayEquals(File(source, path).readBytes(), File(destination, path).readBytes())
        }
        assertFalse(File(destination, "tvserver.sqlite").exists())
        assertFalse(File(destination, "config").exists())
        assertTrue(File(source, "tvserver.sqlite").exists())
    }

    @Test fun failedCopyRemovesItsPartialDestinationAndKeepsTheOriginalBytes() {
        val original = media("movies/clip.mp4", "original content")
        assertThrows(IllegalStateException::class.java) {
            MediaStorageMigration.copy(source, destination) { error("Card removed") }
        }
        assertEquals("original content", original.readText())
        assertFalse(destination.exists())
    }

    @Test fun refusesToOverwriteAnExistingDestination() {
        destination.mkdir()
        val unrelated = File(destination, "keep").apply { writeText("keep") }
        assertThrows(IllegalArgumentException::class.java) { MediaStorageMigration.copy(source, destination) }
        assertEquals("keep", unrelated.readText())
    }

    @Test fun insufficientSpaceKeepsTheLibraryAndCleansTheNewDirectory() {
        val original = media("movies/clip.mp4")
        val fullCard = object : File(destination.path) { override fun getUsableSpace() = 0L }
        assertThrows(IllegalStateException::class.java) { MediaStorageMigration.copy(source, fullCard) }
        assertTrue(original.isFile)
        assertFalse(destination.exists())
    }

    @Test fun rejectsSymbolicLinksWithoutCopyingOutsideTheLibrary() {
        media("movies/normal.mp4")
        val outside = File(root, "private").apply { writeText("secret") }
        Files.createSymbolicLink(File(source, "movies/linked.mp4").toPath(), outside.toPath())
        assertThrows(IllegalStateException::class.java) { MediaStorageMigration.copy(source, destination) }
        assertFalse(destination.exists())
        assertEquals("secret", outside.readText())
    }

    @Test fun cleanupRemovesOnlyMediaAndNeverFollowsDirectoryLinks() {
        media("movies/clip.mp4")
        val database = media("tvserver.sqlite")
        val preferences = media("files/media-storage.json")
        val outside = File(root, "outside").apply { mkdir() }
        val privateFile = File(outside, "keep").apply { writeText("private") }
        Files.createSymbolicLink(File(source, "movies/linked-folder").toPath(), outside.toPath())
        assertTrue(MediaStorageMigration.removeMedia(source))
        assertFalse(File(source, "movies").exists())
        assertTrue(database.isFile)
        assertTrue(preferences.isFile)
        assertEquals("private", privateFile.readText())
    }

    @Test fun emptyLibraryCanMoveAndCreatesBothRoots() {
        MediaStorageMigration.copy(source, destination)
        assertTrue(File(destination, "movies").isDirectory)
        assertTrue(File(destination, "books").isDirectory)
    }
}
