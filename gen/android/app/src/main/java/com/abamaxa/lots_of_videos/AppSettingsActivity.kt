package com.abamaxa.lots_of_videos

import android.content.Intent
import android.content.res.ColorStateList
import android.graphics.Color
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.graphics.drawable.RippleDrawable
import android.graphics.drawable.StateListDrawable
import android.os.Bundle
import android.os.Process
import android.text.InputType
import android.text.SpannableString
import android.text.Spanned
import android.text.format.Formatter
import android.text.style.ForegroundColorSpan
import android.text.style.RelativeSizeSpan
import android.text.style.TypefaceSpan
import android.view.Gravity
import android.view.View
import android.widget.EditText
import android.widget.ImageButton
import android.widget.LinearLayout
import android.widget.ProgressBar
import android.widget.ScrollView
import android.widget.TextView
import androidx.appcompat.app.AlertDialog
import androidx.appcompat.app.AppCompatActivity
import androidx.core.view.AccessibilityDelegateCompat
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.accessibility.AccessibilityNodeInfoCompat
import androidx.core.widget.doAfterTextChanged
import com.google.android.material.button.MaterialButton
import com.google.android.material.dialog.MaterialAlertDialogBuilder
import com.google.android.material.shape.MaterialShapeDrawable
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
        val scroll = ScrollView(this).apply {
            setBackgroundColor(getColor(R.color.settings_background))
            isFillViewport = true
        }
        content = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(20), dp(20), dp(20), dp(32))
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
            isAppearanceLightStatusBars = false
            isAppearanceLightNavigationBars = false
        }
        return content
    }

    private fun text(value: String, heading: Boolean = false): TextView = TextView(this).apply {
        text = value
        textSize = if (heading) 20f else 14f
        setTextColor(getColor(if (heading) R.color.settings_text else R.color.settings_muted))
        setPadding(0, dp(if (heading) 28 else 8), 0, dp(8))
        setLineSpacing(dp(3).toFloat(), 1f)
        if (heading) {
            typeface = Typeface.create("sans-serif-medium", Typeface.NORMAL)
            letterSpacing = -0.02f
            ViewCompat.setAccessibilityHeading(this, true)
        }
        content.addView(this)
    }

    private fun shape(color: Int, radius: Int, border: Int = Color.TRANSPARENT) = GradientDrawable().apply {
        setColor(color)
        cornerRadius = dp(radius).toFloat()
        setStroke(dp(1), border)
    }

    private fun header() {
        val row = LinearLayout(this).apply { gravity = Gravity.CENTER_VERTICAL }
        row.addView(ImageButton(this).apply {
            contentDescription = "Back to library"
            setImageResource(R.drawable.settings_back)
            setPadding(dp(12), dp(12), dp(12), dp(12))
            background = RippleDrawable(ColorStateList.valueOf(getColor(R.color.settings_ripple)),
                shape(getColor(R.color.settings_raised), 24), null)
            setOnClickListener { finish() }
        }, LinearLayout.LayoutParams(dp(44), dp(44)))
        row.addView(TextView(this).apply {
            text = "Settings"
            textSize = 28f
            typeface = Typeface.create("sans-serif-medium", Typeface.NORMAL)
            letterSpacing = -0.04f
            setTextColor(getColor(R.color.settings_text))
            ViewCompat.setAccessibilityHeading(this, true)
        }, LinearLayout.LayoutParams(0, -2, 1f).apply { marginStart = dp(16) })
        content.addView(row)
        content.addView(View(this).apply { setBackgroundColor(getColor(R.color.settings_border)) },
            LinearLayout.LayoutParams(-1, dp(1)).apply { topMargin = dp(24) })
    }

    private fun card(): LinearLayout = LinearLayout(this).apply {
        orientation = LinearLayout.VERTICAL
        background = shape(getColor(R.color.settings_panel), 24, getColor(R.color.settings_border))
        clipToOutline = true
        content.addView(this, LinearLayout.LayoutParams(-1, -2).apply { topMargin = dp(12); bottomMargin = dp(8) })
    }

    private fun preference(parent: LinearLayout, label: String, value: String, action: () -> Unit) {
        if (parent.childCount > 0) {
            parent.addView(View(this).apply { setBackgroundColor(getColor(R.color.settings_border)) },
                LinearLayout.LayoutParams(-1, dp(1)).apply { marginStart = dp(16); marginEnd = dp(16) })
        }
        parent.addView(MaterialButton(this).apply {
            text = SpannableString("$label\n$value").apply {
                setSpan(TypefaceSpan("sans-serif-medium"), 0, label.length, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
                setSpan(ForegroundColorSpan(getColor(R.color.settings_muted)), label.length + 1, length, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
                setSpan(RelativeSizeSpan(14f / 15f), label.length + 1, length, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
            }
            isAllCaps = false
            textSize = 15f
            letterSpacing = 0f
            elevation = 0f
            stateListAnimator = null
            setTextColor(getColor(R.color.settings_text))
            gravity = Gravity.START or Gravity.CENTER_VERTICAL
            setLineSpacing(dp(3).toFloat(), 1f)
            minHeight = dp(76)
            minimumWidth = 0
            insetTop = 0
            insetBottom = 0
            cornerRadius = 0
            backgroundTintList = ColorStateList(
                arrayOf(intArrayOf(android.R.attr.state_focused), intArrayOf()),
                intArrayOf(getColor(R.color.settings_raised), Color.TRANSPARENT))
            rippleColor = ColorStateList.valueOf(getColor(R.color.settings_ripple))
            setPadding(dp(16), dp(14), dp(16), dp(14))
            setIconResource(R.drawable.settings_chevron)
            iconTint = ColorStateList.valueOf(getColor(R.color.settings_muted))
            iconGravity = MaterialButton.ICON_GRAVITY_END
            iconPadding = dp(16)
            setOnClickListener { runCatching(action).onFailure { showError(it) } }
        }, LinearLayout.LayoutParams(-1, -2))
    }

    private fun button(label: String, destructive: Boolean = false, action: () -> Unit): MaterialButton = MaterialButton(this).apply {
        text = label
        isAllCaps = false
        textSize = 15f
        letterSpacing = 0f
        elevation = 0f
        stateListAnimator = null
        minHeight = dp(48)
        insetTop = 0
        insetBottom = 0
        cornerRadius = dp(24)
        setTextColor(getColor(if (destructive) R.color.settings_danger else R.color.settings_mint))
        backgroundTintList = ColorStateList.valueOf(getColor(R.color.settings_panel))
        strokeColor = ColorStateList.valueOf(getColor(if (destructive) R.color.settings_danger else R.color.settings_border))
        strokeWidth = dp(1)
        rippleColor = ColorStateList.valueOf(getColor(R.color.settings_ripple))
        setPadding(dp(20), dp(12), dp(20), dp(12))
        setOnClickListener { runCatching(action).onFailure { showError(it) } }
        content.addView(this, LinearLayout.LayoutParams(-1, -2).apply { topMargin = dp(12) })
    }

    private fun showSettings() {
        panel()
        header()
        runCatching {
            val current = defaults
            text("Playback defaults", true)
            text("Used when starting a new playback session. Controls in the player can change the current session. A playlist's remembered Watch/Listen choice takes precedence.")
            val playback = card()
            preference(playback, "Playback speed", "${current.rate}×") {
                val rates = listOf(0.5f, 0.75f, 1f, 1.25f, 1.5f, 1.75f, 2f)
                choose("Default playback speed", rates.map { "${it}×" }, rates.indexOf(current.rate)) { index ->
                    save { it.put("rate", rates[index].toDouble()) }
                }
            }
            preference(playback, "Audio sync", "${current.audioDelayMs} ms") {
                edit("Default audio sync", current.audioDelayMs.toString(), "Milliseconds, from −5000 to 5000. Negative values play audio earlier.",
                    InputType.TYPE_CLASS_NUMBER or InputType.TYPE_NUMBER_FLAG_SIGNED) { value ->
                    val delay = value.toLongOrNull()
                    require(delay != null && delay in -5000..5000) { "Enter a value from −5000 to 5000" }
                    save { it.put("audioDelayMs", delay) }
                }
            }
            preference(playback, "Audio language", current.audioLanguage.ifEmpty { "From source" }) {
                language("Preferred audio language", current.audioLanguage, "audioLanguage")
            }
            preference(playback, "Subtitles", if (current.subtitles) "From source / preferred language" else "Off") {
                choose("Default subtitles", listOf("From source / preferred language", "Off"), if (current.subtitles) 0 else 1) {
                    save { value -> value.put("subtitles", it == 0) }
                }
            }
            preference(playback, "Subtitle language", current.subtitleLanguage.ifEmpty { "From source" }) {
                language("Preferred subtitle language", current.subtitleLanguage, "subtitleLanguage")
            }
            for ((label, key, mode) in listOf(Triple("Videos", "videoMode", current.videoMode), Triple("Audio", "audioMode", current.audioMode))) {
                preference(playback, label, if (mode == NativePlaybackMode.WATCH) "Watch" else "Listen") {
                    choose("Default mode for ${label.lowercase()}", listOf("Watch · stop when leaving the player", "Listen · continue in the background"),
                        if (mode == NativePlaybackMode.WATCH) 0 else 1) {
                        save { value -> value.put(key, if (it == 0) "watch" else "listen") }
                    }
                }
            }
            preference(playback, "Player display", if (current.displayMode == "video") "Video" else "Visualizer") {
                choose("Default player display", listOf("Video", "Visualizer"), if (current.displayMode == "video") 0 else 1) {
                    save { value -> value.put("displayMode", if (it == 0) "video" else "visualizer") }
                }
            }
            text("Language preferences select an available track; when no matching language exists, the source's default is used.")
            button("Restore playback defaults") {
                showDialog(MaterialAlertDialogBuilder(this).setTitle("Restore playback defaults?")
                    .setMessage("Restore the starting settings for new sessions. Saved playlist modes are kept.")
                    .setNegativeButton("Cancel", null).setPositiveButton("Restore") { _, _ ->
                        runCatching {
                            save { value ->
                                val standard = NativePlaybackDefaults().toJson()
                                standard.keys().forEach { key -> value.put(key, standard.get(key)) }
                            }
                        }.onFailure { showError(it) }
                    })
            }

            text("Storage & data", true)
            val volume = NativeStorage.volumeFor(this)
            text("Moving storage transfers videos, audio, books, thumbnails and unfinished downloads. Playlists and playback history are kept. Keep the SD card inserted while using its library.")
            val storage = card()
            val storageLabel = volume?.let { "${it.label} · ${Formatter.formatFileSize(this, it.directory.usableSpace)} free" } ?: "SD card unavailable"
            preference(storage, "Change media storage", storageLabel) { chooseStorage() }
            button("Erase all data", destructive = true) { confirmErase() }
            text("Erase removes downloaded media, playlists, history, saved logins, settings and caches from this app. Copies exported to other folders are kept.")
        }.onFailure { showError(it) }
    }

    private fun save(change: (JSONObject) -> Unit) {
        NativeSettingsStore.update(this, NativePlaybackDefaults.STORE, change)
        showSettings()
    }

    private fun choose(title: String, labels: List<String>, selected: Int, action: (Int) -> Unit) {
        showDialog(MaterialAlertDialogBuilder(this).setTitle(title).setSingleChoiceItems(labels.toTypedArray(), selected) { dialog, index ->
            runCatching { action(index); dialog.dismiss() }.onFailure { showError(it) }
        }.setNegativeButton("Cancel", null))
    }

    private fun showDialog(builder: MaterialAlertDialogBuilder, destructive: Boolean = false, onShow: (AlertDialog) -> Unit = {}): AlertDialog {
        (builder.background as? MaterialShapeDrawable)?.setStroke(dp(1).toFloat(), getColor(R.color.settings_border))
        val dialog = builder.create()
        dialog.setOnShowListener {
            if (destructive) {
                dialog.getButton(AlertDialog.BUTTON_POSITIVE).backgroundTintList = ColorStateList.valueOf(getColor(R.color.settings_danger))
            }
            onShow(dialog)
        }
        dialog.show()
        dialog.window?.setDimAmount(0.72f)
        return dialog
    }

    private fun edit(title: String, value: String, hint: String, type: Int = InputType.TYPE_CLASS_TEXT, confirmLabel: String = "Save", destructive: Boolean = false, action: (String) -> Unit) {
        val input = EditText(this).apply {
            inputType = type
            setText(value)
            setSingleLine()
            textSize = 16f
            setTextColor(getColor(R.color.settings_text))
            setHintTextColor(getColor(R.color.settings_muted))
            minHeight = dp(48)
            setPadding(dp(16), dp(12), dp(16), dp(12))
            background = StateListDrawable().apply {
                addState(intArrayOf(android.R.attr.state_activated),
                    shape(getColor(R.color.settings_background), 16, getColor(R.color.settings_danger)))
                addState(intArrayOf(android.R.attr.state_focused),
                    shape(getColor(R.color.settings_background), 16, getColor(R.color.settings_mint)))
                addState(intArrayOf(), shape(getColor(R.color.settings_background), 16, getColor(R.color.settings_border)))
            }
        }
        val errorMessage = TextView(this).apply {
            textSize = 14f
            setTextColor(getColor(R.color.settings_danger))
            setLineSpacing(dp(3).toFloat(), 1f)
            visibility = View.GONE
            accessibilityLiveRegion = View.ACCESSIBILITY_LIVE_REGION_POLITE
        }
        ViewCompat.setAccessibilityDelegate(input, object : AccessibilityDelegateCompat() {
            override fun onInitializeAccessibilityNodeInfo(host: View, info: AccessibilityNodeInfoCompat) {
                super.onInitializeAccessibilityNodeInfo(host, info)
                info.isContentInvalid = errorMessage.visibility == View.VISIBLE
                if (info.isContentInvalid) info.error = errorMessage.text
            }
        })
        input.doAfterTextChanged {
            errorMessage.visibility = View.GONE
            input.isActivated = false
        }
        val wrapper = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(24), dp(8), dp(24), 0)
            addView(TextView(context).apply {
                text = hint
                textSize = 14f
                setTextColor(getColor(R.color.settings_muted))
                setLineSpacing(dp(3).toFloat(), 1f)
            })
            addView(input, LinearLayout.LayoutParams(-1, -2).apply { topMargin = dp(16); bottomMargin = dp(8) })
            addView(errorMessage, LinearLayout.LayoutParams(-1, -2).apply { bottomMargin = dp(8) })
        }
        showDialog(MaterialAlertDialogBuilder(this).setTitle(title).setView(wrapper)
            .setNegativeButton("Cancel", null).setPositiveButton(confirmLabel, null), destructive) { dialog ->
            dialog.getButton(AlertDialog.BUTTON_POSITIVE).setOnClickListener {
                runCatching { action(input.text.toString().trim()); dialog.dismiss() }
                    .onFailure {
                        input.isActivated = true
                        errorMessage.text = it.message ?: "The change could not be saved"
                        errorMessage.visibility = View.VISIBLE
                    }
            }
        }
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
            showDialog(MaterialAlertDialogBuilder(this).setTitle("Move media to ${destination.label}?")
                .setMessage("Playback will stop and the app will reopen after your library has been copied and verified. Keep the app open and both storage locations available until the move finishes.")
                .setNegativeButton("Cancel", null).setPositiveButton("Move media") { _, _ -> move(destination) })
        }
    }

    private fun move(volume: NativeStorage.Volume) {
        moving = true
        panel()
        text("Moving media", true)
        content.addView(ProgressBar(this).apply {
            indeterminateTintList = ColorStateList.valueOf(getColor(R.color.settings_mint))
        }, LinearLayout.LayoutParams(dp(48), dp(48)).apply { gravity = Gravity.CENTER_HORIZONTAL; topMargin = dp(24); bottomMargin = dp(16) })
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
        edit("Erase all data?", "", "This permanently erases this app's media, playlists, history, logins and settings, including its SD card library. Keep its SD card inserted. The app will close. Type ERASE to confirm.", confirmLabel = "Erase all data", destructive = true) {
            require(it == "ERASE") { "Type ERASE to confirm" }
            NativeStorage.erase(this)
        }
    }

    private fun showError(error: Throwable) {
        showDialog(MaterialAlertDialogBuilder(this).setTitle("Settings").setMessage(error.message ?: "The change could not be saved")
            .setPositiveButton("OK", null))
    }
}
