package com.abamaxa.lots_of_videos

import android.content.ClipData
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.webkit.MimeTypeMap
import androidx.core.content.FileProvider
import org.json.JSONObject
import java.io.File

/** Android storage boundary: share read-only content URIs and export user-selected copies. */
object NativeVideoFiles {
    fun libraryFile(context: Context, request: JSONObject): File {
        val root = File(context.filesDir.parentFile, "movies").canonicalFile
        val file = File(request.getString("path")).canonicalFile
        check(file.isFile && file.path.startsWith(root.path + File.separator)) {
            "Video is outside the library or unavailable"
        }
        return file
    }

    fun mimeType(file: File): String = MimeTypeMap.getSingleton()
        .getMimeTypeFromExtension(file.extension.lowercase()) ?: "application/octet-stream"

    fun shareIntent(context: Context, request: JSONObject): Intent {
        val files = request.getJSONArray("files")
        check(files.length() > 0) { "No videos found" }
        val sources = (0 until files.length()).map { libraryFile(context, files.getJSONObject(it)) }
        val uris = ArrayList(sources.map {
            FileProvider.getUriForFile(context, "${context.packageName}.fileprovider", it)
        })
        val types = sources.map { mimeType(it) }.distinct()
        val intent = Intent(if (uris.size == 1) Intent.ACTION_SEND else Intent.ACTION_SEND_MULTIPLE)
        intent.type = if (types.size == 1) types[0] else "*/*"
        if (uris.size == 1) intent.putExtra(Intent.EXTRA_STREAM, uris[0])
        else intent.putParcelableArrayListExtra(Intent.EXTRA_STREAM, uris)
        intent.clipData = ClipData.newUri(context.contentResolver, sources[0].name, uris[0]).apply {
            uris.drop(1).forEach { addItem(ClipData.Item(it)) }
        }
        return intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
    }

    fun exportIntent(file: File): Intent = Intent(Intent.ACTION_CREATE_DOCUMENT)
        .addCategory(Intent.CATEGORY_OPENABLE)
        .setType(mimeType(file))
        .putExtra(Intent.EXTRA_TITLE, file.name)

    fun copyTo(context: Context, source: File, destination: Uri) {
        source.inputStream().use { input ->
            val output = context.contentResolver.openOutputStream(destination, "w")
                ?: error("Could not open the selected destination")
            output.use { input.copyTo(it) }
        }
    }
}
