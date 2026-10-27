package com.abamaxa.lots_of_videos

import android.Manifest
import android.app.Activity
import android.app.AlertDialog
import android.app.Dialog
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.content.pm.PackageManager
import android.graphics.Color
import android.graphics.drawable.ColorDrawable
import android.os.Bundle
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.view.Gravity
import android.view.MotionEvent
import android.view.View
import android.view.WindowManager
import android.widget.FrameLayout
import android.widget.Button
import android.widget.LinearLayout
import android.widget.SeekBar
import android.widget.TextView
import android.widget.Toast
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import org.json.JSONObject
import org.videolan.libvlc.util.VLCVideoLayout

/** A reconnectable window onto NativePlaybackService. It never owns a decoder or an audio source. */
class NativePlayerActivity : Activity() {
    private lateinit var root: FrameLayout
    private lateinit var surface: VLCVideoLayout
    private lateinit var chrome: NativePlayerChrome
    private var visualizer: NativeVisualizerView? = null
    private var service: NativePlaybackService? = null
    private var request: JSONObject? = null
    private var resumed = false
    private var bound = false
    private var dragging = false
    private var controlDialogOpen = false
    private var settingsDialog: Dialog? = null
    private var requestingAnalysisPermission = false
    private var state = JSONObject()
    private val handler = Handler(Looper.getMainLooper())
    private val listener: (JSONObject) -> Unit = { update(it) }
    private val hideControls = Runnable {
        if (!state.optBoolean("paused", true) && !dragging && !controlDialogOpen) {
            chrome.visibility = View.INVISIBLE
        }
    }
    private val connection = object : ServiceConnection {
        override fun onServiceConnected(name: ComponentName, binder: IBinder) {
            val playback = (binder as NativePlaybackService.LocalBinder).service
            service = playback
            if (resumed) {
                playback.attachSurface(surface)
                playback.setUiVisible(true)
            }
            request?.let { playback.open(it) }
            request = null
            playback.addListener(listener)
            if (playback.snapshot().optString("videoId").isEmpty()) finish()
        }
        override fun onServiceDisconnected(name: ComponentName) {
            service = null
            finish()
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        WindowCompat.setDecorFitsSystemWindows(window, false)
        volumeControlStream = android.media.AudioManager.STREAM_MUSIC
        intent.getStringExtra("video")?.let { request = JSONObject(it) }
        root = FrameLayout(this).apply {
            setBackgroundColor(Color.BLACK)
            contentDescription = "Video playback"
            setOnClickListener { toggleControls() }
        }
        surface = VLCVideoLayout(this)
        root.addView(surface, FrameLayout.LayoutParams(-1, -1))
        chrome = NativePlayerChrome(this, ::playerAction)
        chrome.progress.setOnSeekBarChangeListener(object : SeekBar.OnSeekBarChangeListener {
            override fun onStartTrackingTouch(seekBar: SeekBar) {
                dragging = true
                handler.removeCallbacks(hideControls)
            }
            override fun onProgressChanged(seekBar: SeekBar, value: Int, fromUser: Boolean) {}
            override fun onStopTrackingTouch(seekBar: SeekBar) {
                service?.seekTo((state.optDouble("duration") * seekBar.progress).toLong())
                dragging = false
                showControls()
            }
        })
        root.addView(chrome, FrameLayout.LayoutParams(-1, -1))
        ViewCompat.setOnApplyWindowInsetsListener(root) { _, insets ->
            chrome.applyInsets(insets.getInsetsIgnoringVisibility(
                WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout()))
            insets
        }
        setContentView(root)
        ViewCompat.requestApplyInsets(root)
        hideSystemBars()
        startService(Intent(this, NativePlaybackService::class.java))
        bound = bindService(Intent(this, NativePlaybackService::class.java), connection, Context.BIND_AUTO_CREATE)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        intent.getStringExtra("video")?.let {
            val next = JSONObject(it)
            if (service != null) service?.open(next) else request = next
        }
        showControls()
    }

    private fun chooseVisualizer() {
        if (checkSelfPermission(Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED) {
            service?.setDisplayMode("visualizer")
        } else {
            // Keep Video selected until output-analysis permission has actually been granted.
            chrome.setDisplayMode("video")
            dialogOpened()
            AlertDialog.Builder(this).setTitle("Visualize playback audio")
                .setMessage("Android requires audio permission to analyze this player's output. The visualizer uses only the playing file; it does not use the microphone.")
                .setPositiveButton("Continue") { _, _ ->
                    requestingAnalysisPermission = true
                    requestPermissions(arrayOf(Manifest.permission.RECORD_AUDIO), ANALYSIS_PERMISSION)
                }.setNegativeButton("Cancel", null).setOnDismissListener { dialogClosed() }.show()
        }
    }

    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode == ANALYSIS_PERMISSION) {
            requestingAnalysisPermission = false
            if (grantResults.firstOrNull() == PackageManager.PERMISSION_GRANTED) service?.setDisplayMode("visualizer")
            else Toast.makeText(this, "Audio permission is needed for the visualizer", Toast.LENGTH_LONG).show()
        }
    }

    private fun update(next: JSONObject) {
        val wasPaused = state.optBoolean("paused", true)
        state = next
        if (next.optBoolean("closed")) {
            next.optString("error").takeIf { it.isNotEmpty() && it != "null" }?.let {
                Toast.makeText(this, it, Toast.LENGTH_LONG).show()
            }
            if (!isFinishing) finish()
            return
        }
        chrome.bind(next, dragging)
        val visualizing = next.optString("displayMode") == "visualizer"
        if (visualizing && visualizer == null) {
            visualizer = NativeVisualizerView(this) { message ->
                service?.setDisplayMode("video")
                Toast.makeText(this, "Visualizer unavailable: $message", Toast.LENGTH_LONG).show()
            }.also { view ->
                view.setOnTouchListener { _, event ->
                    if (event.action == MotionEvent.ACTION_UP) toggleControls()
                    true
                }
                root.addView(view, 1, FrameLayout.LayoutParams(-1, -1))
            }
        }
        visualizer?.visibility = if (visualizing) View.VISIBLE else View.GONE
        visualizer?.setPlayback(visualizing && resumed && next.optBoolean("playing"), next.optInt("audioSessionId"))
        if (next.optBoolean("paused", true)) showControls()
        else if (wasPaused) scheduleControlsHide()
    }

    private fun control(action: String) { service?.control(JSONObject().put("action", action)) }

    private fun playerAction(action: NativePlayerChrome.Action) {
        when (action) {
            NativePlayerChrome.Action.BACK -> finish()
            NativePlayerChrome.Action.STOP -> service?.stopPlayback()
            NativePlayerChrome.Action.TOGGLE -> control("togglePause")
            NativePlayerChrome.Action.REWIND -> service?.seek(-10)
            NativePlayerChrome.Action.FORWARD -> service?.seek(30)
            NativePlayerChrome.Action.PREVIOUS -> control("previous")
            NativePlayerChrome.Action.NEXT -> control("next")
            NativePlayerChrome.Action.VIDEO -> service?.setDisplayMode("video")
            NativePlayerChrome.Action.VISUALIZER -> chooseVisualizer()
            NativePlayerChrome.Action.BACKGROUND -> service?.setBackgroundAudio(chrome.backgroundAudio.isChecked)
            NativePlayerChrome.Action.SETTINGS -> showSettings()
            NativePlayerChrome.Action.AUDIO -> chooseTrack(false)
            NativePlayerChrome.Action.SUBTITLES -> chooseTrack(true)
            NativePlayerChrome.Action.SPEED -> chooseSpeed()
            NativePlayerChrome.Action.SYNC -> chooseAudioSync()
        }
        showControls()
    }

    private fun showSettings() {
        val audio = service?.tracks(false)?.firstOrNull { it.id == state.optInt("currentAudioTrack", -1) }?.name ?: "Default"
        val subtitles = if (state.optInt("currentSubtitleTrack", -1) < 0) "Off" else
            service?.tracks(true)?.firstOrNull { it.id == state.optInt("currentSubtitleTrack") }?.name ?: "On"
        val speed = state.optDouble("playbackRate", 1.0).toString().removeSuffix(".0") + "×"
        val dialog = Dialog(this)
        settingsDialog = dialog
        dialog.setContentView(chrome.settingsContent(audio, subtitles, speed, audioSyncLabel(state.optLong("audioDelayMs")), { selected ->
            dialog.dismiss()
            playerAction(selected)
        }, { dialog.dismiss() }))
        dialog.setCanceledOnTouchOutside(true)
        dialog.setOnDismissListener { settingsDialog = null; dialogClosed() }
        dialogOpened()
        dialog.show()
        dialog.window?.apply {
            setBackgroundDrawable(ColorDrawable(Color.TRANSPARENT))
            addFlags(WindowManager.LayoutParams.FLAG_DIM_BEHIND)
            setDimAmount(0.3f)
            setLayout(minOf(chrome.dp(328), root.width - chrome.safeInsets.left - chrome.safeInsets.right - chrome.dp(32)), -2)
            attributes = attributes.apply {
                gravity = Gravity.TOP or Gravity.RIGHT
                x = chrome.safeInsets.right + chrome.dp(16)
                y = chrome.safeInsets.top + chrome.dp(16)
            }
            WindowCompat.getInsetsController(this, decorView).hide(WindowInsetsCompat.Type.systemBars())
        }
    }

    private fun chooseSpeed() {
        val rates = floatArrayOf(0.5f, 1f, 1.25f, 1.5f, 2f)
        dialogOpened()
        AlertDialog.Builder(this).setTitle("Playback speed")
            .setSingleChoiceItems(rates.map { it.toString() + "×" }.toTypedArray(),
                rates.indexOfFirst { it == state.optDouble("playbackRate", 1.0).toFloat() }) { dialog, index ->
                service?.control(JSONObject().put("action", "rate").put("rate", rates[index]))
                dialog.dismiss()
            }.setOnDismissListener { dialogClosed() }.show()
    }

    private fun audioSyncLabel(delay: Long): String = when {
        delay < 0 -> "${-delay} ms earlier"
        delay > 0 -> "$delay ms later"
        else -> "Default timing"
    }

    private fun chooseAudioSync() {
        var delay = state.optLong("audioDelayMs")
        val panel = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(chrome.dp(24), chrome.dp(8), chrome.dp(24), chrome.dp(8))
        }
        panel.addView(TextView(this).apply {
            text = "If sound follows the picture, choose Earlier. Your correction is remembered on this device."
        })
        val reading = TextView(this).apply {
            text = audioSyncLabel(delay)
            gravity = Gravity.CENTER
            setPadding(0, chrome.dp(16), 0, chrome.dp(8))
            accessibilityLiveRegion = View.ACCESSIBILITY_LIVE_REGION_POLITE
        }
        panel.addView(reading)
        fun adjust(value: Long) {
            val next = value.coerceIn(-5000L, 5000L)
            if (service?.setAudioDelay(next) == true) {
                delay = next
                reading.text = audioSyncLabel(delay)
            } else Toast.makeText(this, "Audio sync is not available yet", Toast.LENGTH_SHORT).show()
        }
        val buttons = LinearLayout(this).apply { gravity = Gravity.CENTER }
        for ((label, action) in listOf<Pair<String, () -> Unit>>(
            "Earlier" to { adjust(delay - 50) }, "Later" to { adjust(delay + 50) })) {
            buttons.addView(Button(this).apply {
                text = label
                contentDescription = "Sound ${label.lowercase()} by 50 milliseconds"
                setOnClickListener { action() }
            }, LinearLayout.LayoutParams(0, -2, 1f))
        }
        panel.addView(buttons)
        dialogOpened()
        AlertDialog.Builder(this).setTitle("Audio sync").setView(panel)
            .setNeutralButton("Reset") { _, _ -> adjust(0) }
            .setPositiveButton("Done", null)
            .setOnDismissListener { dialogClosed() }.show()
    }

    private fun hideSystemBars() {
        WindowCompat.getInsetsController(window, window.decorView).apply {
            systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
            hide(WindowInsetsCompat.Type.systemBars())
        }
    }

    private fun toggleControls() {
        if (chrome.visibility == View.VISIBLE && !state.optBoolean("paused", true)) {
            handler.removeCallbacks(hideControls)
            chrome.visibility = View.INVISIBLE
        } else showControls()
    }

    private fun scheduleControlsHide() {
        handler.removeCallbacks(hideControls)
        if (!state.optBoolean("paused", true) && !dragging && !controlDialogOpen) handler.postDelayed(hideControls, 3000)
    }

    private fun showControls() {
        chrome.visibility = View.VISIBLE
        scheduleControlsHide()
    }

    private fun dialogOpened() { controlDialogOpen = true; handler.removeCallbacks(hideControls) }
    private fun dialogClosed() { controlDialogOpen = false; scheduleControlsHide() }

    override fun onUserInteraction() { super.onUserInteraction(); scheduleControlsHide() }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (hasFocus) hideSystemBars()
    }

    private fun chooseTrack(subtitles: Boolean) {
        val tracks = service?.tracks(subtitles) ?: return
        if (tracks.isEmpty()) return
        dialogOpened()
        val selected = state.optInt(if (subtitles) "currentSubtitleTrack" else "currentAudioTrack", -1)
        AlertDialog.Builder(this).setTitle(if (subtitles) "Subtitles" else "Audio track")
            .setSingleChoiceItems(tracks.map { it.name }.toTypedArray(), tracks.indexOfFirst { it.id == selected }) { dialog, index ->
                service?.control(JSONObject().put("action", if (subtitles) "subtitle" else "audio").put("track", tracks[index].id))
                dialog.dismiss()
            }.setOnDismissListener { dialogClosed() }.show()
    }

    override fun onResume() {
        super.onResume()
        resumed = true
        service?.attachSurface(surface)
        service?.setUiVisible(true)
        service?.snapshot()?.let { update(it) }
        showControls()
    }

    override fun onPause() {
        resumed = false
        visualizer?.setPlayback(false, state.optInt("audioSessionId"))
        if (!requestingAnalysisPermission) service?.setUiVisible(false)
        super.onPause()
    }

    override fun onStop() {
        service?.setUiVisible(false)
        service?.detachSurface(surface)
        super.onStop()
    }

    override fun onDestroy() {
        settingsDialog?.dismiss()
        handler.removeCallbacksAndMessages(null)
        service?.removeListener(listener)
        service?.detachSurface(surface)
        if (isFinishing && service?.backgroundAudio != true) service?.stopPlayback()
        if (bound) unbindService(connection)
        visualizer?.destroy()
        service = null
        super.onDestroy()
    }

    companion object {
        private const val ANALYSIS_PERMISSION = 1003
    }
}
