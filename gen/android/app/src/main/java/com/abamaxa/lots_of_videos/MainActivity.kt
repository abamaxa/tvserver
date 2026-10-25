package com.abamaxa.lots_of_videos

import android.graphics.Color
import android.app.Activity
import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.provider.DocumentsContract
import android.view.View
import android.widget.LinearLayout
import android.widget.ProgressBar
import android.widget.TextView
import android.widget.Toast
import androidx.activity.result.contract.ActivityResultContracts
import androidx.appcompat.app.AlertDialog
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import org.json.JSONObject
import java.io.File
import kotlin.concurrent.thread

class MainActivity : TauriActivity() {
    private val notificationPermission = registerForActivityResult(ActivityResultContracts.RequestPermission()) { }

    fun requestDownloadNotifications() {
        if (isFinishing || isDestroyed) return
        if (Build.VERSION.SDK_INT < 33 || checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED) return
        val preferences = getSharedPreferences("downloads", MODE_PRIVATE)
        if (preferences.getBoolean("notificationPermissionRequested", false)) return
        preferences.edit().putBoolean("notificationPermissionRequested", true).apply()
        notificationPermission.launch(Manifest.permission.POST_NOTIFICATIONS)
    }

    private var pendingExport: File? = null
    private var copyingExport = false
    private val exportPicker = registerForActivityResult(ActivityResultContracts.StartActivityForResult()) { result ->
        val source = pendingExport
        pendingExport = null
        val destination = result.data?.data
        if (result.resultCode == Activity.RESULT_OK && source != null && destination != null) {
            copyingExport = true
            val content = LinearLayout(this).apply {
                orientation = LinearLayout.HORIZONTAL
                val padding = (24 * resources.displayMetrics.density).toInt()
                setPadding(padding, padding, padding, padding)
                addView(ProgressBar(context))
                addView(TextView(context).apply { text = "  Saving video…" })
            }
            val progress = AlertDialog.Builder(this).setView(content).setCancelable(false).show()
            val appContext = applicationContext
            thread(name = "video-export") {
                val outcome = runCatching { NativeVideoFiles.copyTo(appContext, source, destination) }
                if (outcome.isFailure) {
                    runCatching { DocumentsContract.deleteDocument(appContext.contentResolver, destination) }
                }
                runOnUiThread {
                    copyingExport = false
                    if (!isFinishing && !isDestroyed) {
                        progress.dismiss()
                        val message = outcome.fold({ "Saved ${source.name}" }, { "Could not save video: ${it.message}" })
                        Toast.makeText(this, message, Toast.LENGTH_LONG).show()
                    }
                }
            }
        }
    }

    fun exportVideo(file: File) {
        if (pendingExport != null || copyingExport) {
            Toast.makeText(this, "Finish saving the current video first", Toast.LENGTH_LONG).show()
            return
        }
        pendingExport = file
        try {
            exportPicker.launch(NativeVideoFiles.exportIntent(file))
        } catch (error: Exception) {
            pendingExport = null
            Toast.makeText(this, "Could not open Save dialog: ${error.message}", Toast.LENGTH_LONG).show()
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        savedInstanceState?.getString("pendingVideoExport")?.let { path ->
            pendingExport = runCatching {
                NativeVideoFiles.libraryFile(this, JSONObject().put("path", path))
            }.getOrNull()
        }
        // Android 15+ draws the WebView behind system bars by default. Keep the
        // entire library, including fixed buttons and modal portals, in safe bounds.
        val content = findViewById<View>(android.R.id.content)
        content.setBackgroundColor(Color.WHITE)
        ViewCompat.setOnApplyWindowInsetsListener(content) { view, insets ->
            val safe = insets.getInsets(WindowInsetsCompat.Type.systemBars() or
                WindowInsetsCompat.Type.displayCutout() or WindowInsetsCompat.Type.ime())
            view.setPadding(safe.left, safe.top, safe.right, safe.bottom)
            WindowInsetsCompat.CONSUMED
        }
        ViewCompat.requestApplyInsets(content)
        WindowCompat.getInsetsController(window, window.decorView).apply {
            isAppearanceLightStatusBars = true
            isAppearanceLightNavigationBars = true
        }
    }

    override fun onSaveInstanceState(outState: Bundle) {
        pendingExport?.let { outState.putString("pendingVideoExport", it.absolutePath) }
        super.onSaveInstanceState(outState)
    }
}
