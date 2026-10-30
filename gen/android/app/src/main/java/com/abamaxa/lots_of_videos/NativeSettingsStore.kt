package com.abamaxa.lots_of_videos

import android.content.Context
import android.util.AtomicFile
import org.json.JSONObject
import java.io.File
import java.io.FileOutputStream

/** Read on demand: the settings window lives in a process without the media backend. */
internal object NativeSettingsStore {
    @Synchronized fun read(context: Context, name: String): JSONObject = locked(context, name) { readFile(context, name) }

    private fun readFile(context: Context, name: String): JSONObject {
        val file = AtomicFile(File(context.filesDir, "$name.json"))
        return try {
            file.openRead().use { JSONObject(it.bufferedReader().readText()) }
        } catch (_: java.io.FileNotFoundException) { JSONObject() }
    }

    @Synchronized fun update(context: Context, name: String, change: (JSONObject) -> Unit) {
        locked(context, name) {
            val value = readFile(context, name)
            change(value)
            val file = AtomicFile(File(context.filesDir, "$name.json"))
            val output = file.startWrite()
            try {
                val bytes = value.toString().toByteArray(Charsets.UTF_8)
                output.write(bytes)
                file.finishWrite(output)
                check(file.openRead().use { it.readBytes() }.contentEquals(bytes)) { "Could not save settings" }
            } catch (error: Exception) {
                file.failWrite(output)
                throw error
            }
        }
    }

    private fun <T> locked(context: Context, name: String, action: () -> T): T {
        context.filesDir.mkdirs()
        return FileOutputStream(File(context.filesDir, "$name.lock"), true).channel.use { channel ->
            channel.lock().use { action() }
        }
    }
}
