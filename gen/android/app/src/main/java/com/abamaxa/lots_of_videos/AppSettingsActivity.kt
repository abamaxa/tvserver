package com.abamaxa.lots_of_videos

import android.content.Intent
import android.graphics.Color
import android.os.Bundle
import android.os.Process
import android.text.InputType
import android.text.format.Formatter
import android.widget.Button
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.ProgressBar
import android.widget.ScrollView
import android.widget.TextView
import androidx.appcompat.app.AlertDialog
import androidx.appcompat.app.AppCompatActivity
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import org.json.JSONObject
import java.io.File
import kotlin.concurrent.thread

/** Kept in :settings so a library move can stop every backend writer before touching files. */
class AppSettingsActivity : AppCompatActivity() {
    private lateinit var content: LinearLayout
    private var moving = false
    private val defaults get() = NativePlaybackDefaults.load(this)
    private fun dp(value: Int) = (value * resources.displayMetrics.density).toInt()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        onBackPressedDispatcher.addCallback(this, object : androidx.activity.OnBackPressedCallback(true) {
            override fun handleOnBackPressed() { if (!moving) finish() }
        })
        showSettings()
    }

    private fun panel(): LinearLayout {
        val scroll = ScrollView(this).apply { setBackgroundColor(Color.rgb(248, 249, 251)) }
        content = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(20), dp(20), dp(20), dp(28))
        }
        scroll.addView(content)
        ViewCompat.setOnApplyWindowInsetsListener(scroll) { view, insets ->
            val safe = insets.getInsets(WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout() or WindowInsetsCompat.Type.ime())
            view.setPadding(safe.left, safe.top, safe.right, safe.bottom)
            WindowInsetsCompat.CONSUMED
        }
        setContentView(scroll)
        ViewCompat.requestApplyInsets(scroll)
        WindowCompat.getInsetsController(window, window.decorView).apply {
            isAppearanceLightStatusBars = true
            isAppearanceLightNavigationBars = true
        }
        return content
    }

    private fun text(value: String, heading: Boolean = false): TextView = TextView(this).apply {
        text = value
        textSize = if (heading) 21f else 14f
        setTextColor(if (heading) Color.rgb(29, 39, 57) else Color.rgb(75, 85, 99))
        setPadding(0, dp(if (heading) 24 else 8), 0, dp(8))
        if (heading) setTypeface(typeface, android.graphics.Typeface.BOLD)
        content.addView(this)
    }

    private fun button(label: String, action: () -> Unit): Button = Button(this).apply {
        text = label
        isAllCaps = false
        minHeight = dp(52)
        setOnClickListener { runCatching(action).onFailure { showError(it) } }
        content.addView(this, LinearLayout.LayoutParams(-1, -2))
    }

    private fun showSettings() {
        panel()
        text("Settings", true)
        button("Back to library") { finish() }
        runCatching {
            val current = defaults
            text("Playback defaults", true)
            text("Used when starting a new playback session. Controls in the player can change the current session. A playlist's remembered Watch/Listen choice takes precedence.")
            button("Playback speed · ${current.rate}×") {
                val rates = listOf(0.5f, 0.75f, 1f, 1.25f, 1.5f, 1.75f, 2f)
                choose("Default playback speed", rates.map { "${it}×" }, rates.indexOf(current.rate)) { index ->
                    save { it.put("rate", rates[index].toDouble()) }
                }
            }
            button("Audio sync · ${current.audioDelayMs} ms") {
                edit("Default audio sync", current.audioDelayMs.toString(), "Milliseconds, from −5000 to 5000. Negative values play audio earlier.",
                    InputType.TYPE_CLASS_NUMBER or InputType.TYPE_NUMBER_FLAG_SIGNED) { value ->
                    val delay = value.toLongOrNull()
                    require(delay != null && delay in -5000..5000) { "Enter a value from −5000 to 5000" }
                    save { it.put("audioDelayMs", delay) }
                }
            }
            button("Audio language · ${current.audioLanguage.ifEmpty { "From source" }}") {
                language("Preferred audio language", current.audioLanguage, "audioLanguage")
            }
            button("Subtitles · ${if (current.subtitles) "From source / preferred language" else "Off"}") {
                choose("Default subtitles", listOf("From source / preferred language", "Off"), if (current.subtitles) 0 else 1) {
                    save { value -> value.put("subtitles", it == 0) }
                }
            }
            button("Subtitle language · ${current.subtitleLanguage.ifEmpty { "From source" }}") {
                language("Preferred subtitle language", current.subtitleLanguage, "subtitleLanguage")
            }
            text("Language preferences select an available track; when no matching language exists, the source's default is used.")
            for ((label, key, mode) in listOf(Triple("Videos", "videoMode", current.videoMode), Triple("Audio", "audioMode", current.audioMode))) {
                button("$label · ${if (mode == NativePlaybackMode.WATCH) "Watch" else "Listen"}") {
                    choose("Default mode for ${label.lowercase()}", listOf("Watch · stop when leaving the player", "Listen · continue in the background"),
                        if (mode == NativePlaybackMode.WATCH) 0 else 1) {
                        save { value -> value.put(key, if (it == 0) "watch" else "listen") }
                    }
                }
            }
            button("Player display · ${if (current.displayMode == "video") "Video" else "Visualizer"}") {
                choose("Default player display", listOf("Video", "Visualizer"), if (current.displayMode == "video") 0 else 1) {
                    save { value -> value.put("displayMode", if (it == 0) "video" else "visualizer") }
                }
            }
            button("Restore playback defaults") {
                AlertDialog.Builder(this).setTitle("Restore playback defaults?")
                    .setMessage("Restore the starting settings for new sessions. Saved playlist modes are kept.")
                    .setNegativeButton("Cancel", null).setPositiveButton("Restore") { _, _ ->
                        runCatching {
                            save { value ->
                                val standard = NativePlaybackDefaults().toJson()
                                standard.keys().forEach { key -> value.put(key, standard.get(key)) }
                            }
                        }.onFailure { showError(it) }
                    }.show()
            }

            text("Storage & data", true)
            val volume = NativeStorage.volumeFor(this)
            text("Media storage: ${volume?.label ?: "SD card unavailable"}")
            if (volume != null) text("${Formatter.formatFileSize(this, volume.directory.usableSpace)} free")
            text("Moving storage transfers videos, audio, books, thumbnails and unfinished downloads. Playlists and playback history are kept. Keep the SD card inserted while using its library.")
            button("Change media storage") { chooseStorage() }
            button("Erase all data") { confirmErase() }.setTextColor(Color.rgb(180, 30, 40))
            text("Erase removes downloaded media, playlists, history, saved logins, settings and caches from this app. Copies exported to other folders are kept.")
        }.onFailure { showError(it) }
    }

    private fun save(change: (JSONObject) -> Unit) {
        NativeSettingsStore.update(this, NativePlaybackDefaults.STORE, change)
        showSettings()
    }

    private fun choose(title: String, labels: List<String>, selected: Int, action: (Int) -> Unit) {
        AlertDialog.Builder(this).setTitle(title).setSingleChoiceItems(labels.toTypedArray(), selected) { dialog, index ->
            runCatching { action(index); dialog.dismiss() }.onFailure { showError(it) }
        }.setNegativeButton("Cancel", null).show()
    }

    private fun edit(title: String, value: String, hint: String, type: Int = InputType.TYPE_CLASS_TEXT, confirmLabel: String = "Save", action: (String) -> Unit) {
        val input = EditText(this).apply { setText(value); inputType = type; setSingleLine() }
        val wrapper = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(24), dp(8), dp(24), 0)
            addView(TextView(context).apply { text = hint })
            addView(input)
        }
        val dialog = AlertDialog.Builder(this).setTitle(title).setView(wrapper)
            .setNegativeButton("Cancel", null).setPositiveButton(confirmLabel, null).create()
        dialog.setOnShowListener {
            dialog.getButton(AlertDialog.BUTTON_POSITIVE).setOnClickListener {
                runCatching { action(input.text.toString().trim()); dialog.dismiss() }
                    .onFailure { input.error = it.message }
            }
        }
        dialog.show()
    }

    private fun language(title: String, current: String, key: String) {
        edit(title, current, "Language code, e.g. en / eng or es / spa. Leave empty to use the source's choice.") { value ->
            NativePlaybackDefaults.vlcLanguage(value)
            save { it.put(key, value) }
        }
    }

    private fun chooseStorage() {
        check(!intent.getBooleanExtra("downloadsActive", false)) { "Finish or cancel running tasks, then reopen Settings to move storage" }
        check(NativeStorage.available(this)) { "Reinsert the current SD card before moving the library" }
        val volumes = NativeStorage.volumes(this)
        if (volumes.size == 1 && NativeStorage.volumeFor(this)?.directory == NativeStorage.internalRoot(this)) {
            text("No writable SD card found. Insert a card, then reopen Settings.")
            return
        }
        choose("Media storage", volumes.map { "${it.label} · ${Formatter.formatFileSize(this, it.directory.usableSpace)} free" },
            volumes.indexOfFirst { it.directory == NativeStorage.volumeFor(this)?.directory }) { index ->
            val destination = volumes[index]
            if (destination.directory == NativeStorage.volumeFor(this)?.directory) return@choose
            AlertDialog.Builder(this).setTitle("Move media to ${destination.label}?")
                .setMessage("Playback will stop and the app will reopen after your library has been copied and verified. Keep the app open and both storage locations available until the move finishes.")
                .setNegativeButton("Cancel", null).setPositiveButton("Move media") { _, _ -> move(destination) }.show()
        }
    }

    private fun move(volume: NativeStorage.Volume) {
        moving = true
        panel()
        text("Moving media", true)
        content.addView(ProgressBar(this))
        val progress = text("Preparing library…")
        thread(name = "media-storage-move") {
            val result = runCatching {
                NativeStorage.withMaintenance(this) {
                    val destination = NativeStorage.planMove(this, volume)
                    val backend = getSystemService(android.app.ActivityManager::class.java).runningAppProcesses
                        ?.firstOrNull { it.processName == packageName && it.uid == Process.myUid() }?.pid ?: -1
                    if (backend > 0 && backend != Process.myPid()) {
                        Process.killProcess(backend)
                        // Do not copy while a terminated backend can still hold writers open.
                        val deadline = android.os.SystemClock.elapsedRealtime() + 5000
                        while (File("/proc/$backend").exists() && android.os.SystemClock.elapsedRealtime() < deadline) Thread.sleep(25)
                        check(!File("/proc/$backend").exists()) { "Could not stop the media backend" }
                    }
                    NativeStorage.move(this, destination) { name -> runOnUiThread { progress.text = "Copying and verifying $name" } }
                }
            }
            runOnUiThread {
                moving = false
                if (result.isSuccess) {
                    startActivity(Intent(this, StorageLauncherActivity::class.java))
                    finish()
                } else {
                    panel()
                    text("Media could not be moved", true)
                    text(result.exceptionOrNull()?.message ?: "Storage is unavailable")
                    text("The original library has been kept. Reopen the app to return to it.")
                    button("Reopen app") { startActivity(Intent(this, StorageLauncherActivity::class.java)); finish() }
                }
            }
        }
    }

    private fun confirmErase() {
        edit("Erase all data?", "", "This permanently erases this app's media, playlists, history, logins and settings, including its SD card library. Keep its SD card inserted. The app will close. Type ERASE to confirm.", confirmLabel = "Erase all data") {
            require(it == "ERASE") { "Type ERASE to confirm" }
            NativeStorage.erase(this)
        }
    }

    private fun showError(error: Throwable) {
        AlertDialog.Builder(this).setTitle("Settings").setMessage(error.message ?: "The change could not be saved")
            .setPositiveButton("OK", null).show()
    }
}
