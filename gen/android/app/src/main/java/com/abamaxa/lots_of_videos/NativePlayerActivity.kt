package com.abamaxa.lots_of_videos

import android.app.Activity
import android.app.AlertDialog
import android.graphics.Color
import android.net.Uri
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.view.Gravity
import android.view.View
import android.view.WindowManager
import android.widget.Button
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.SeekBar
import android.widget.TextView
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import org.json.JSONObject
import org.videolan.libvlc.LibVLC
import org.videolan.libvlc.Media
import org.videolan.libvlc.MediaPlayer
import org.videolan.libvlc.util.VLCVideoLayout
import java.io.File

/** In-process LibVLC player: MKV demuxing and E-AC-3 audio do not depend on WebView codecs. */
class NativePlayerActivity : Activity() {
    private lateinit var engine: LibVLC
    private lateinit var playback: MediaPlayer
    private lateinit var media: Media
    private lateinit var details: JSONObject
    private lateinit var progress: SeekBar
    private lateinit var clock: TextView
    private lateinit var toggle: Button
    private lateinit var header: LinearLayout
    private lateinit var controls: LinearLayout
    private val handler = Handler(Looper.getMainLooper())
    private var dragging = false
    private var controlDialogOpen = false
    private var startPosition = 0L
    private var errorMessage: String? = null
    private val hideControls = Runnable {
        if (::playback.isInitialized && playback.isPlaying && !dragging) {
            header.visibility = View.INVISIBLE
            controls.visibility = View.INVISIBLE
        }
    }
    private val ticker = object : Runnable {
        override fun run() {
            if (!::playback.isInitialized) return
            val length = playback.length.coerceAtLeast(0)
            val time = playback.time.coerceAtLeast(0)
            if (!dragging) progress.progress = if (length > 0) (time * 1000 / length).toInt() else 0
            clock.text = "${formatTime(time)} / ${formatTime(length)}"
            toggle.text = if (playback.isPlaying) "Pause" else "Play"
            toggle.contentDescription = toggle.text
            publish()
            handler.postDelayed(this, 1000)
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        WindowCompat.setDecorFitsSystemWindows(window, false)
        volumeControlStream = android.media.AudioManager.STREAM_MUSIC
        details = JSONObject(intent.getStringExtra("video") ?: "{}")
        startPosition = (details.optDouble("startTime", 0.0).coerceAtLeast(0.0) * 1000).toLong()
        val root = FrameLayout(this).apply {
            setBackgroundColor(Color.BLACK)
            contentDescription = "Video playback"
            setOnClickListener {
                if (header.visibility == View.VISIBLE) {
                    handler.removeCallbacks(hideControls)
                    header.visibility = View.INVISIBLE
                    controls.visibility = View.INVISIBLE
                } else showControls()
            }
        }
        val surface = VLCVideoLayout(this)
        root.addView(surface, FrameLayout.LayoutParams(-1, -1))
        header = LinearLayout(this).apply {
            gravity = Gravity.CENTER_VERTICAL
            setBackgroundColor(0xCC000000.toInt())
            addView(button("Back") { finish() })
            addView(TextView(this@NativePlayerActivity).apply {
                text = details.optString("video")
                setTextColor(Color.WHITE)
                maxLines = 2
            }, LinearLayout.LayoutParams(0, -2, 1f))
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
                    if (::playback.isInitialized && playback.isSeekable) {
                        playback.setTime(playback.length * seekBar.progress / 1000)
                    }
                    dragging = false
                    showControls()
                    publish()
                }
            })
        }
        val transport = LinearLayout(this).apply { gravity = Gravity.CENTER }
        transport.addView(button("−10 s") { seek(-10) })
        toggle = button("Pause") { if (playback.isPlaying) playback.pause() else playback.play(); publish() }
        transport.addView(toggle)
        transport.addView(button("+30 s") { seek(30) })
        val tracks = LinearLayout(this).apply { gravity = Gravity.CENTER }
        tracks.addView(button("Audio") { chooseTrack(false) })
        tracks.addView(button("Subtitles") { chooseTrack(true) })
        tracks.addView(button("Speed") {
            val rates = floatArrayOf(0.5f, 1f, 1.25f, 1.5f, 2f)
            controlDialogOpen = true
            handler.removeCallbacks(hideControls)
            AlertDialog.Builder(this).setTitle("Playback speed")
                .setItems(rates.map { "${it}×" }.toTypedArray()) { _, index -> playback.rate = rates[index] }
                .setOnDismissListener { controlDialogOpen = false; scheduleControlsHide() }.show()
        })
        controls.addView(clock)
        controls.addView(progress)
        controls.addView(transport)
        controls.addView(tracks)
        root.addView(controls, FrameLayout.LayoutParams(-1, -2, Gravity.BOTTOM))
        ViewCompat.setOnApplyWindowInsetsListener(root) { _, insets ->
            // Pad only the controls. Video fills the display even behind cutouts
            // and temporarily revealed system bars.
            val safe = insets.getInsetsIgnoringVisibility(
                WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout())
            header.setPadding(safe.left, safe.top, safe.right, 0)
            controls.setPadding(safe.left, 0, safe.right, safe.bottom)
            insets
        }
        setContentView(root)
        ViewCompat.requestApplyInsets(root)
        hideSystemBars()
        NativeBridge.player = this
        try {
            engine = LibVLC(this, arrayListOf("--audio-time-stretch", "--no-video-title-show"))
            playback = MediaPlayer(engine)
            playback.attachViews(surface, null, true, false)
            playback.setEventListener { event ->
                when (event.type) {
                    MediaPlayer.Event.Playing -> {
                        if (startPosition > 0 && playback.isSeekable) {
                            playback.setTime(startPosition)
                            startPosition = 0
                        }
                        scheduleControlsHide()
                        publish()
                    }
                    MediaPlayer.Event.EncounteredError -> {
                        errorMessage = "This video could not be played."
                        publish()
                        AlertDialog.Builder(this).setMessage(errorMessage)
                            .setPositiveButton("Back") { _, _ -> finish() }.show()
                    }
                    MediaPlayer.Event.Paused, MediaPlayer.Event.EndReached -> {
                        showControls()
                        publish()
                    }
                }
            }
            media = Media(engine, Uri.fromFile(File(details.getString("path"))))
            media.setHWDecoderEnabled(true, false)
            playback.media = media
            playback.play()
            handler.post(ticker)
        } catch (error: Exception) {
            errorMessage = error.message ?: "Native player initialization failed"
            publish()
            AlertDialog.Builder(this).setMessage(errorMessage)
                .setPositiveButton("Back") { _, _ -> finish() }.show()
        }
    }

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

    private fun scheduleControlsHide() {
        handler.removeCallbacks(hideControls)
        if (::playback.isInitialized && playback.isPlaying && !dragging && !controlDialogOpen) {
            handler.postDelayed(hideControls, 3000)
        }
    }

    private fun showControls() {
        header.visibility = View.VISIBLE
        controls.visibility = View.VISIBLE
        scheduleControlsHide()
    }

    override fun onUserInteraction() {
        super.onUserInteraction()
        scheduleControlsHide()
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (hasFocus) hideSystemBars()
    }

    private fun seek(seconds: Int) {
        if (playback.isSeekable) playback.setTime((playback.time + seconds * 1000L).coerceIn(0, playback.length.coerceAtLeast(0)))
        publish()
    }

    fun control(request: JSONObject) {
        if (!::playback.isInitialized) return
        when (request.optString("action")) {
            "seek" -> seek(request.optInt("seconds"))
            "togglePause" -> if (playback.isPlaying) playback.pause() else playback.play()
            "pause" -> playback.pause()
            "stop" -> finish()
            "subtitle" -> playback.setSpuTrack(request.optInt("track", -1))
            "rate" -> playback.rate = request.optDouble("rate", 1.0).toFloat()
        }
        showControls()
        publish()
    }

    private fun chooseTrack(subtitles: Boolean) {
        val tracks = (if (subtitles) playback.spuTracks else playback.audioTracks) ?: emptyArray()
        if (tracks.isEmpty()) return
        controlDialogOpen = true
        handler.removeCallbacks(hideControls)
        AlertDialog.Builder(this).setTitle(if (subtitles) "Subtitles" else "Audio track")
            .setItems(tracks.map { it.name }.toTypedArray()) { _, index ->
                if (subtitles) playback.setSpuTrack(tracks[index].id) else playback.setAudioTrack(tracks[index].id)
                publish()
            }.setOnDismissListener { controlDialogOpen = false; scheduleControlsHide() }.show()
    }

    private fun publish(closed: Boolean = false) {
        if (!::details.isInitialized) return
        val ready = ::playback.isInitialized
        val stats = if (::media.isInitialized) media.stats else null
        NativeBridge.publish(JSONObject()
            .put("videoId", details.optString("videoId"))
            .put("video", details.optString("video"))
            .put("collection", details.optString("collection"))
            .put("currentSrc", details.optString("path"))
            .put("currentTime", if (ready) playback.time.coerceAtLeast(0) / 1000.0 else 0.0)
            .put("duration", if (ready) playback.length.coerceAtLeast(0) / 1000.0 else 0.0)
            .put("paused", !ready || !playback.isPlaying || closed)
            .put("playbackRate", if (ready) playback.rate.toDouble() else 1.0)
            .put("currentSubtitleTrack", if (ready) playback.spuTrack else -1)
            .put("audioTracks", if (ready) playback.audioTracksCount else 0)
            .put("decodedAudio", stats?.decodedAudio ?: 0)
            .put("decodedVideo", stats?.decodedVideo ?: 0)
            .put("closed", closed)
            .put("error", errorMessage ?: JSONObject.NULL))
    }

    override fun onPause() {
        if (::playback.isInitialized) { playback.pause(); publish() }
        super.onPause()
    }

    override fun onDestroy() {
        handler.removeCallbacksAndMessages(null)
        publish(closed = true)
        if (NativeBridge.player === this) NativeBridge.player = null
        if (::playback.isInitialized) {
            playback.setEventListener(null)
            playback.stop()
            playback.detachViews()
            playback.release()
        }
        if (::media.isInitialized) media.release()
        if (::engine.isInitialized) engine.release()
        super.onDestroy()
    }

    private fun formatTime(ms: Long): String {
        val seconds = ms / 1000
        return "%02d:%02d:%02d".format(seconds / 3600, seconds / 60 % 60, seconds % 60)
    }
}
