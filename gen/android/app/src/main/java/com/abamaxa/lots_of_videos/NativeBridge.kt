package com.abamaxa.lots_of_videos

import android.app.Activity
import android.content.Context
import android.content.Intent
import android.os.Handler
import android.os.Looper
import android.util.Log
import com.yausername.ffmpeg.FFmpeg
import com.yausername.youtubedl_android.YoutubeDL
import org.json.JSONObject
import java.io.File
import java.security.MessageDigest
import android.system.Os
import android.widget.Toast

/** JNI boundary shared by Rust's download tasks and the native player. */
object NativeBridge {
    @Volatile var playback: NativePlaybackService? = null
    @Volatile var playerState = "{}"

    @JvmStatic external fun nativePlayerEvent(state: String)
    @JvmStatic external fun nativeDownloadStopped(id: String, reason: String)

    @JvmStatic
    fun beginDownload(activity: Activity, payload: String): String {
        activity.runOnUiThread { (activity as? MainActivity)?.requestDownloadNotifications() }
        return NativeDownloads.begin(activity.applicationContext, payload)
    }

    @JvmStatic
    fun updateDownload(payload: String): String {
        NativeDownloads.update(payload)
        return "{}"
    }

    @JvmStatic
    fun finishDownload(payload: String): String {
        NativeDownloads.end(payload)
        return "{}"
    }

    internal fun downloadStopped(id: String, reason: String) {
        try {
            nativeDownloadStopped(id, reason)
        } catch (error: UnsatisfiedLinkError) {
            Log.e("LotsOfVideosDownloads", "Native download bridge is unavailable", error)
        }
    }

    @JvmStatic
    @Synchronized
    fun prepareRuntime(context: Context): String {
        // JNI-attached Rust workers have no application context class loader.
        // The downloader SDK's archive helpers use it for reflective Java I/O.
        val thread = Thread.currentThread()
        val previous = thread.contextClassLoader
        try {
            thread.contextClassLoader = context.classLoader
            return prepareRuntimeWithClassLoader(context)
        } finally { thread.contextClassLoader = previous }
    }

    private fun prepareRuntimeWithClassLoader(context: Context): String {
        // Executables stay in the installed, read-only native library directory.
        // Only the Python standard library, certificates and yt-dlp data are unpacked.
        YoutubeDL.getInstance().init(context.applicationContext)
        FFmpeg.getInstance().init(context.applicationContext)
        val bin = File(context.applicationInfo.nativeLibraryDir)
        val base = File(context.noBackupFilesDir, "youtubedl-android")
        val packages = File(base, "packages")
        val pythonHome = File(packages, "python/usr")
        val environment = JSONObject()
            .put("PYTHONHOME", pythonHome.absolutePath)
            .put("LD_LIBRARY_PATH", listOf(
                File(pythonHome, "lib"), File(packages, "ffmpeg/usr/lib")
            ).joinToString(":") { it.absolutePath })
            .put("SSL_CERT_FILE", File(pythonHome, "etc/tls/cert.pem").absolutePath)
            .put("PATH", "${System.getenv("PATH")}:${bin.absolutePath}")
            .put("TMPDIR", context.cacheDir.absolutePath)
        val python = File(bin, "libpython.so")
        val ffmpeg = File(bin, "libffmpeg.so")
        val quickjs = File(bin, "libqjs.so")
        val script = File(base, "yt-dlp/yt-dlp")
        for (file in listOf(python, ffmpeg, quickjs, script)) {
            check(file.isFile) { "Bundled runtime is missing ${file.name}" }
        }
        val release = JSONObject(context.assets.open("ytdlp/version.json").bufferedReader().use { it.readText() })
        val marker = File(base, "yt-dlp/bundled-sha256")
        if (!marker.isFile || marker.readText() != release.getString("sha256")) {
            val temporary = File(script.parentFile, "yt-dlp.new")
            context.assets.open("ytdlp/yt-dlp").use { source -> temporary.outputStream().use { source.copyTo(it) } }
            val digest = MessageDigest.getInstance("SHA-256").digest(temporary.readBytes())
                .joinToString("") { "%02x".format(it) }
            check(digest == release.getString("sha256")) { "Bundled yt-dlp checksum does not match" }
            check(temporary.renameTo(script)) { "Could not install bundled yt-dlp" }
            marker.writeText(digest)
        }
        // Official yt-dlp expects these conventional binary names. The symlinks
        // target executables installed by Android; writable code is never executed.
        val mediaBin = File(base, "media-bin").apply { mkdirs() }
        for ((name, library) in mapOf("python" to "libpython.so", "quickjs" to "libqjs.so",
            "ffmpeg" to "libffmpeg.so", "ffprobe" to "libffprobe.so")) {
            val destination = File(mediaBin, name)
            try { Os.remove(destination.absolutePath) } catch (error: android.system.ErrnoException) {
                if (error.errno != android.system.OsConstants.ENOENT) throw error
            }
            Os.symlink(File(bin, library).absolutePath, destination.absolutePath)
        }
        // APK installation paths contain '='. Android's env treats those paths as
        // assignments, so expose every executable through a stable app-private link.
        return JSONObject().put("python", File(mediaBin, "python").absolutePath)
            .put("ffmpeg", File(mediaBin, "ffmpeg").absolutePath)
            .put("ffprobe", File(mediaBin, "ffprobe").absolutePath)
            .put("quickjs", File(mediaBin, "quickjs").absolutePath).put("version", release.getString("version"))
            .put("script", script.absolutePath).put("environment", environment).toString()
    }

    @JvmStatic
    fun openPlayer(activity: Activity, payload: String): String {
        activity.runOnUiThread {
            activity.startActivity(Intent(activity, NativePlayerActivity::class.java)
                .addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP)
                .putExtra("video", payload))
        }
        return "{}"
    }

    @JvmStatic
    fun shareVideos(activity: Activity, payload: String): String {
        val intent = NativeVideoFiles.shareIntent(activity, JSONObject(payload))
        activity.runOnUiThread {
            try {
                activity.startActivity(Intent.createChooser(intent, "Share video"))
            } catch (error: Exception) {
                Toast.makeText(activity, "Could not share video: ${error.message}", Toast.LENGTH_LONG).show()
            }
        }
        return "{}"
    }

    @JvmStatic
    fun exportVideo(activity: Activity, payload: String): String {
        val file = NativeVideoFiles.libraryFile(activity, JSONObject(payload))
        activity.runOnUiThread { (activity as MainActivity).exportVideo(file) }
        return "{}"
    }

    @JvmStatic
    fun controlPlayer(payload: String): String {
        val request = JSONObject(payload)
        Handler(Looper.getMainLooper()).post { playback?.control(request) }
        return playerState
    }

    fun publish(state: JSONObject) {
        playerState = state.toString()
        try {
            nativePlayerEvent(playerState)
        } catch (error: UnsatisfiedLinkError) {
            Log.e("LotsOfVideosPlayer", "Native event bridge is unavailable", error)
        }
    }
}
