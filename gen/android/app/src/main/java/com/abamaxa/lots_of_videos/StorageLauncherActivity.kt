package com.abamaxa.lots_of_videos

import android.content.Intent
import android.graphics.Color
import android.os.Bundle
import android.widget.Button
import android.widget.LinearLayout
import android.widget.TextView
import androidx.appcompat.app.AppCompatActivity
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import kotlin.concurrent.thread

/** Check removable storage before loading Rust: an absent card must never look like an empty library. */
class StorageLauncherActivity : AppCompatActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val panel = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setBackgroundColor(Color.rgb(248, 249, 251))
        }
        ViewCompat.setOnApplyWindowInsetsListener(panel) { view, insets ->
            val safe = insets.getInsets(WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout())
            val padding = (20 * resources.displayMetrics.density).toInt()
            view.setPadding(safe.left + padding, safe.top + padding, safe.right + padding, safe.bottom + padding)
            WindowInsetsCompat.CONSUMED
        }
        val message = TextView(this).apply {
            text = "Opening media library…"
            textSize = 18f
            setTextColor(Color.rgb(29, 39, 57))
        }
        panel.addView(message)
        setContentView(panel)
        ViewCompat.requestApplyInsets(panel)
        WindowCompat.getInsetsController(window, window.decorView).apply {
            isAppearanceLightStatusBars = true
            isAppearanceLightNavigationBars = true
        }
        thread(name = "storage-recovery") {
            val result = runCatching {
                NativeStorage.recover(this)
                check(NativeStorage.available(this)) { "Reinsert your SD card to open this library. Your library and playlists have been kept." }
            }
            runOnUiThread {
                if (isFinishing || isDestroyed) return@runOnUiThread
                if (result.isSuccess) {
                    startActivity(Intent(this, MainActivity::class.java))
                    finish()
                } else {
                    message.text = result.exceptionOrNull()?.message ?: "Media storage is unavailable"
                    panel.addView(Button(this).apply {
                        text = "Try again"
                        setOnClickListener { recreate() }
                    })
                    panel.addView(Button(this).apply {
                        text = "Settings"
                        setOnClickListener { startActivity(Intent(this@StorageLauncherActivity, AppSettingsActivity::class.java)) }
                    })
                }
            }
        }
    }
}
