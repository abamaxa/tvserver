package com.abamaxa.lots_of_videos

import android.app.ActivityManager
import android.content.Context
import android.os.Environment
import java.io.File
import java.io.FileOutputStream
import java.util.UUID

internal object NativeStorage {
    private const val STORE = "media-storage"
    data class Volume(val label: String, val directory: File)

    fun <T> withMaintenance(context: Context, action: () -> T): T =
        FileOutputStream(File(context.filesDir, "media-maintenance.lock"), true).channel.use { channel ->
            val lock = try { channel.tryLock() } catch (_: java.nio.channels.OverlappingFileLockException) { null }
            check(lock != null) { "A media move is in progress. Wait for it to finish, then reopen the app." }
            lock.use { action() }
        }

    fun internalRoot(context: Context): File = checkNotNull(context.filesDir.parentFile).canonicalFile

    fun volumes(context: Context): List<Volume> = listOf(Volume("Internal storage", internalRoot(context))) +
        context.getExternalFilesDirs(null).filterNotNull().filter {
            Environment.isExternalStorageRemovable(it) && Environment.getExternalStorageState(it) == Environment.MEDIA_MOUNTED
        }.mapIndexed { index, directory ->
            Volume(if (index == 0) "SD card" else "SD card ${index + 1}", directory.canonicalFile)
        }

    fun mediaRoot(context: Context): File {
        val saved = NativeSettingsStore.read(context, STORE).optString("root")
        return if (saved.isEmpty()) internalRoot(context) else File(saved)
    }

    fun volumeFor(context: Context, root: File = mediaRoot(context)): Volume? = volumes(context).firstOrNull {
        root == it.directory || root.path.startsWith(it.directory.path + File.separator)
    }

    fun available(context: Context): Boolean = volumeFor(context) != null && mediaRoot(context).isDirectory &&
        !NativeSettingsStore.read(context, STORE).has("pendingRoot")

    fun requireMediaRoot(context: Context): File {
        check(available(context)) { "Media storage is unavailable. Reinsert the SD card and reopen the app." }
        return mediaRoot(context)
    }

    fun planMove(context: Context, volume: Volume): File {
        check(available(context)) { "Insert the current SD card before moving its library" }
        check(volumes(context).any { it.directory == volume.directory }) { "This storage is no longer available" }
        val destination = File(volume.directory, "media-libraries/${UUID.randomUUID()}")
        NativeSettingsStore.update(context, STORE) {
            it.put("pendingRoot", destination.absolutePath)
        }
        return destination
    }

    fun move(context: Context, destination: File, progress: (String) -> Unit) {
        val source = requireNotNull(volumeFor(context)) { "The source storage is unavailable" }
            .let { mediaRoot(context) }
        check(volumeFor(context, destination) != null) { "The destination storage is unavailable" }
        check(destination.parentFile!!.mkdirs() || destination.parentFile!!.isDirectory)
        MediaStorageMigration.copy(source, destination, progress)
        NativeSettingsStore.update(context, STORE) {
            check(it.getString("pendingRoot") == destination.path) { "The storage request changed" }
            it.put("root", destination.path).put("previousRoot", source.path).remove("pendingRoot")
        }
        finishCleanup(context)
    }

    /** A killed/interrupted copy never changes the active library. Resume cleanup before backend startup. */
    fun recover(context: Context) {
        withMaintenance(context) {
            val pending = NativeSettingsStore.read(context, STORE).optString("pendingRoot")
            if (pending.isNotEmpty()) {
                val directory = File(pending)
                check(volumeFor(context, directory) != null) { "Reinsert the SD card to clean up the interrupted move" }
                check(directory.deleteRecursively() || !directory.exists()) { "Could not clean up the interrupted move" }
                NativeSettingsStore.update(context, STORE) { it.remove("pendingRoot") }
            }
            finishCleanup(context)
        }
    }

    private fun finishCleanup(context: Context) {
        if (!available(context)) return
        val previous = NativeSettingsStore.read(context, STORE).optString("previousRoot")
        if (previous.isEmpty()) return
        val root = File(previous)
        if (volumeFor(context, root) != null && MediaStorageMigration.removeMedia(root)) {
            NativeSettingsStore.update(context, STORE) { it.remove("previousRoot") }
        }
    }

    fun erase(context: Context) {
        val saved = NativeSettingsStore.read(context, STORE)
        val roots = listOf("root", "pendingRoot", "previousRoot").map { saved.optString(it) }.filter { it.isNotEmpty() }
        check(roots.all { volumeFor(context, File(it)) != null }) {
            "Insert the SD cards containing this app's media before erasing its data"
        }
        // Android stops this app's processes before clearing SQLite, WebView, preferences,
        // caches and the app's private directories on internal and external volumes.
        check(context.getSystemService(ActivityManager::class.java).clearApplicationUserData()) {
            "Android could not erase app data. Try Clear storage in Android's app settings."
        }
    }

}
