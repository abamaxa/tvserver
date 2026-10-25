package com.abamaxa.lots_of_videos

import android.Manifest
import android.app.Activity
import android.app.AlertDialog
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.content.pm.PackageManager
import android.graphics.Color
import android.os.Bundle
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.view.Gravity
import android.view.MotionEvent
import android.view.View
import android.view.WindowManager
import android.widget.Button
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.RadioButton
import android.widget.RadioGroup
import android.widget.SeekBar
import android.widget.Switch
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
    private lateinit var progress: SeekBar
    private lateinit var clock: TextView
    private lateinit var title: TextView
    private lateinit var toggle: Button
    private lateinit var header: LinearLayout
    private lateinit var controls: LinearLayout
    private lateinit var background: Switch
    private lateinit var display: RadioGroup
    private var visualizer: NativeVisualizerView? = null
    private var service: NativePlaybackService? = null
    private var request: JSONObject? = null
    private var resumed = false
    private var bound = false
    private var dragging = false
    private var updatingUi = false
    private var controlDialogOpen = false
    private var requestingAnalysisPermission = false
    private var state = JSONObject()
    private val handler = Handler(Looper.getMainLooper())
    private val listener: (JSONObject) -> Unit = { update(it) }
    private val hideControls = Runnable {
        if (!state.optBoolean("paused", true) && !dragging && !controlDialogOpen) {
            header.visibility = View.INVISIBLE
            controls.visibility = View.INVISIBLE
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
        header = LinearLayout(this).apply {
            gravity = Gravity.CENTER_VERTICAL
            setBackgroundColor(0xCC000000.toInt())
            addView(button("Back") { finish() })
            title = TextView(this@NativePlayerActivity).apply {
                setTextColor(Color.WHITE)
                maxLines = 2
            }
            addView(title, LinearLayout.LayoutParams(0, -2, 1f))
            addView(button("Stop") { service?.stopPlayback() })
        }
        root.addView(header, FrameLayout.LayoutParams(-1, -2, Gravity.TOP))
        controls = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setBackgroundColor(0xDD000000.toInt())
        }
        clock = TextView(this).apply { setTextColor(Color.WHITE); gravity = Gravity.CENTER }
        progress = SeekBar(this).apply {
            max = 1000
            contentDescription = "Video position"
            setOnSeekBarChangeListener(object : SeekBar.OnSeekBarChangeListener {
                override fun onStartTrackingTouch(seekBar: SeekBar) {
                    dragging = true
                    handler.removeCallbacks(hideControls)
                }
                override fun onProgressChanged(seekBar: SeekBar, value: Int, fromUser: Boolean) {}
                override fun onStopTrackingTouch(seekBar: SeekBar) {
                    service?.seekTo((state.optDouble("duration") * 1000 * seekBar.progress / 1000).toLong())
                    dragging = false
                    showControls()
                }
            })
        }
        val transport = LinearLayout(this).apply { gravity = Gravity.CENTER }
        transport.addView(button("−10 s") { service?.seek(-10) })
        toggle = button("Pause") { control("togglePause") }
        transport.addView(toggle)
        transport.addView(button("+30 s") { service?.seek(30) })
        val tracks = LinearLayout(this).apply { gravity = Gravity.CENTER }
        tracks.addView(button("Audio") { chooseTrack(false) })
        tracks.addView(button("Subtitles") { chooseTrack(true) })
        tracks.addView(button("Speed") {
            val rates = floatArrayOf(0.5f, 1f, 1.25f, 1.5f, 2f)
            dialogOpened()
            AlertDialog.Builder(this).setTitle("Playback speed")
                .setSingleChoiceItems(rates.map { "${it}×" }.toTypedArray(),
                    rates.indexOfFirst { it == state.optDouble("playbackRate", 1.0).toFloat() }) { dialog, index ->
                    service?.control(JSONObject().put("action", "rate").put("rate", rates[index]))
                    dialog.dismiss()
                }.setOnDismissListener { dialogClosed() }.show()
        })
        val options = LinearLayout(this).apply { gravity = Gravity.CENTER }
        display = RadioGroup(this).apply {
            orientation = RadioGroup.HORIZONTAL
            contentDescription = "Playback display"
            addView(RadioButton(context).apply { id = VIDEO; text = "Video"; setTextColor(Color.WHITE) })
            addView(RadioButton(context).apply { id = VISUALIZER; text = "Visualizer"; setTextColor(Color.WHITE) })
            check(VIDEO)
            setOnCheckedChangeListener { _, selected ->
                if (!updatingUi) {
                    if (selected == VISUALIZER) chooseVisualizer() else service?.setDisplayMode("video")
                    showControls()
                }
            }
        }
        options.addView(display)
        background = Switch(this).apply {
            text = "Keep audio playing with screen off"
            contentDescription = text
            setTextColor(Color.WHITE)
            setPadding(24, 0, 16, 0)
            setOnCheckedChangeListener { _, checked ->
                if (!updatingUi) { service?.setBackgroundAudio(checked); showControls() }
            }
        }
        options.addView(background)
        controls.addView(clock)
        controls.addView(progress)
        controls.addView(transport)
        controls.addView(tracks)
        controls.addView(options)
        root.addView(controls, FrameLayout.LayoutParams(-1, -2, Gravity.BOTTOM))
        ViewCompat.setOnApplyWindowInsetsListener(root) { _, insets ->
            val safe = insets.getInsetsIgnoringVisibility(
                WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout())
            header.setPadding(safe.left, safe.top, safe.right, 0)
            controls.setPadding(safe.left, 0, safe.right, safe.bottom)
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
            // Reset the radio until permission has actually been granted.
            updatingUi = true
            display.check(VIDEO)
            updatingUi = false
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
        updatingUi = true
        title.text = next.optString("video")
        background.isChecked = next.optBoolean("backgroundAudio")
        val visualizing = next.optString("displayMode") == "visualizer"
        display.check(if (visualizing) VISUALIZER else VIDEO)
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
        val time = (next.optDouble("currentTime") * 1000).toLong()
        val length = (next.optDouble("duration") * 1000).toLong()
        if (!dragging) progress.progress = if (length > 0) (time * 1000 / length).toInt() else 0
        clock.text = "${formatTime(time)} / ${formatTime(length)}"
        toggle.text = if (next.optBoolean("paused", true)) "Play" else "Pause"
        toggle.contentDescription = toggle.text
        updatingUi = false
        if (next.optBoolean("paused", true)) showControls()
        else if (wasPaused) scheduleControlsHide()
    }

    private fun control(action: String) { service?.control(JSONObject().put("action", action)) }

    private fun button(label: String, action: () -> Unit) = Button(this).apply {
        text = label
        contentDescription = label
        setOnClickListener { action(); showControls() }
    }

    private fun hideSystemBars() {
        WindowCompat.getInsetsController(window, window.decorView).apply {
            systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
            hide(WindowInsetsCompat.Type.systemBars())
        }
    }

    private fun toggleControls() {
        if (header.visibility == View.VISIBLE && !state.optBoolean("paused", true)) {
            handler.removeCallbacks(hideControls)
            header.visibility = View.INVISIBLE
            controls.visibility = View.INVISIBLE
        } else showControls()
    }

    private fun scheduleControlsHide() {
        handler.removeCallbacks(hideControls)
        if (!state.optBoolean("paused", true) && !dragging && !controlDialogOpen) handler.postDelayed(hideControls, 3000)
    }

    private fun showControls() {
        header.visibility = View.VISIBLE
        controls.visibility = View.VISIBLE
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
        handler.removeCallbacksAndMessages(null)
        service?.removeListener(listener)
        service?.detachSurface(surface)
        if (isFinishing && service?.backgroundAudio != true) service?.stopPlayback()
        if (bound) unbindService(connection)
        visualizer?.destroy()
        service = null
        super.onDestroy()
    }

    private fun formatTime(ms: Long): String {
        val seconds = ms / 1000
        return "%02d:%02d:%02d".format(seconds / 3600, seconds / 60 % 60, seconds % 60)
    }

    companion object {
        private const val VIDEO = 1001
        private const val VISUALIZER = 1002
        private const val ANALYSIS_PERMISSION = 1003
    }
}
