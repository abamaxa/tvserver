package com.abamaxa.lots_of_videos

import java.io.File
import java.io.FileOutputStream
import java.io.IOException
import java.security.MessageDigest

/** Runs after the backend process exits. The source survives until the durable pointer switches. */
internal object MediaStorageMigration {
    private val libraries = listOf("movies", "books")

    fun copy(source: File, destination: File, progress: (String) -> Unit = {}) {
        require(source.canonicalFile != destination.canonicalFile) { "Storage is already selected" }
        require(!destination.exists()) { "The destination must be a new library" }
        check(destination.mkdirs()) { "Could not create the new library" }
        try {
            val roots = libraries.map { File(source.canonicalFile, it).canonicalRoot() }
            val bytes = roots.filter { it.exists() }.fold(0L) { total, root -> Math.addExact(total, size(root)) }
            check(destination.usableSpace >= bytes + 16L * 1024 * 1024) { "There is not enough free space to move this library" }
            roots.forEach { root ->
                val target = File(destination, root.name)
                if (root.exists()) copyTree(root, target, progress) else check(target.mkdir())
            }
        } catch (error: Exception) {
            destination.deleteRecursively()
            throw error
        }
    }

    /** Only media is removed; the legacy root also contains SQLite, preferences and WebView data. */
    fun removeMedia(root: File): Boolean = libraries.map { name -> removeTree(File(root.canonicalFile, name)) }.all { it }

    private fun removeTree(file: File): Boolean {
        if (file.canonicalFile != file.absoluteFile) return file.delete()
        if (!file.exists()) return true
        if (file.isDirectory && !(file.listFiles()?.map { removeTree(it) }?.all { it } ?: false)) return false
        return file.delete()
    }

    private fun File.canonicalRoot(): File {
        check(canonicalFile == absoluteFile) { "The library contains a symbolic link: $name" }
        return this
    }

    private fun children(directory: File): Array<File> = directory.listFiles()
        ?: throw IOException("Could not read ${directory.name}")

    private fun size(file: File): Long {
        file.canonicalRoot()
        return when {
            file.isDirectory -> children(file).fold(0L) { total, child -> Math.addExact(total, size(child)) }
            file.isFile -> file.length()
            else -> throw IOException("Unsupported library file: ${file.name}")
        }
    }

    private fun copyTree(source: File, destination: File, progress: (String) -> Unit) {
        source.canonicalRoot()
        if (source.isDirectory) {
            check(destination.mkdir()) { "Could not create ${destination.name}" }
            children(source).forEach { copyTree(it, File(destination, it.name), progress) }
        } else {
            check(source.isFile) { "Unsupported library file: ${source.name}" }
            progress(source.name)
            val digest = MessageDigest.getInstance("SHA-256")
            source.inputStream().use { input ->
                FileOutputStream(destination).use { output ->
                    val buffer = ByteArray(1024 * 1024)
                    while (true) {
                        val count = input.read(buffer)
                        if (count < 0) break
                        digest.update(buffer, 0, count)
                        output.write(buffer, 0, count)
                    }
                    output.fd.sync()
                }
            }
            check(source.length() == destination.length() && digest.digest().contentEquals(hash(destination))) {
                "Could not verify ${source.name}; the original library has been kept"
            }
        }
    }

    private fun hash(file: File): ByteArray {
        val digest = MessageDigest.getInstance("SHA-256")
        file.inputStream().use { input ->
            val buffer = ByteArray(1024 * 1024)
            while (true) {
                val count = input.read(buffer)
                if (count < 0) break
                digest.update(buffer, 0, count)
            }
        }
        return digest.digest()
    }
}
