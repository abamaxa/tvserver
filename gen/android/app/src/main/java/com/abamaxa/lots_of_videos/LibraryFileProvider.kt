package com.abamaxa.lots_of_videos

import android.content.ContentProvider
import android.content.ContentValues
import android.content.Context
import android.content.pm.ProviderInfo
import android.database.Cursor
import android.database.MatrixCursor
import android.net.Uri
import android.os.ParcelFileDescriptor
import android.provider.OpenableColumns
import org.json.JSONObject
import java.io.File

/** Read-only grants are resolved against the current library on any Android volume. */
class LibraryFileProvider : ContentProvider() {
    override fun onCreate() = true

    override fun attachInfo(context: Context, info: ProviderInfo) {
        check(!info.exported && info.grantUriPermissions) { "Library sharing requires private, explicit URI grants" }
        super.attachInfo(context, info)
    }

    private fun file(uri: Uri): File {
        val context = checkNotNull(context)
        require(uri.authority == "${context.packageName}.fileprovider" && uri.pathSegments.firstOrNull() == "video") {
            "Invalid library URI"
        }
        val relative = uri.pathSegments.drop(1).joinToString(File.separator)
        require(relative.isNotEmpty()) { "No library file selected" }
        val path = File(File(NativeStorage.requireMediaRoot(context), "movies"), relative)
        return NativeVideoFiles.libraryFile(context, JSONObject().put("path", path.path))
    }

    override fun openFile(uri: Uri, mode: String): ParcelFileDescriptor {
        check(mode == "r") { "Shared library files are read-only" }
        return ParcelFileDescriptor.open(file(uri), ParcelFileDescriptor.MODE_READ_ONLY)
    }

    override fun getType(uri: Uri): String = NativeVideoFiles.mimeType(file(uri))

    override fun query(uri: Uri, projection: Array<out String>?, selection: String?, selectionArgs: Array<out String>?, sortOrder: String?): Cursor {
        val file = file(uri)
        val columns = (projection ?: arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE))
            .filter { it == OpenableColumns.DISPLAY_NAME || it == OpenableColumns.SIZE }.toTypedArray()
        return MatrixCursor(columns, 1).apply {
            addRow(columns.map { if (it == OpenableColumns.DISPLAY_NAME) file.name else file.length() })
        }
    }

    override fun insert(uri: Uri, values: ContentValues?): Uri = throw UnsupportedOperationException("Read-only library")
    override fun update(uri: Uri, values: ContentValues?, selection: String?, selectionArgs: Array<out String>?): Int =
        throw UnsupportedOperationException("Read-only library")
    override fun delete(uri: Uri, selection: String?, selectionArgs: Array<out String>?): Int =
        throw UnsupportedOperationException("Read-only library")

    companion object {
        fun uriForFile(context: Context, file: File): Uri {
            val root = File(NativeStorage.requireMediaRoot(context), "movies").canonicalFile
            val source = NativeVideoFiles.libraryFile(context, JSONObject().put("path", file.path))
            return Uri.Builder().scheme("content").authority("${context.packageName}.fileprovider").appendPath("video")
                .apply { source.relativeTo(root).invariantSeparatorsPath.split('/').forEach { appendPath(it) } }.build()
        }
    }
}
