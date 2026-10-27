package com.abamaxa.lots_of_videos

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.ServiceInfo
import android.media.AudioAttributes
import android.media.AudioFocusRequest
import android.media.AudioManager
import android.media.MediaMetadata
import android.media.session.MediaSession
import android.media.session.PlaybackState
import android.net.Uri
import android.os.Binder
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.PowerManager
import androidx.core.content.ContextCompat
import org.json.JSONObject
import org.videolan.libvlc.LibVLC
import org.videolan.libvlc.Media
import org.videolan.libvlc.MediaPlayer
import org.videolan.libvlc.interfaces.IVLCVout
import org.videolan.libvlc.util.VLCVideoLayout
import java.io.File
import java.util.UUID

/** Sole owner of the decoder, sound, media session and progress, independent of any window. */
class NativePlaybackService : Service() {
    inner class LocalBinder : Binder() { val service get() = this@NativePlaybackService }
    private val handler = Handler(Looper.getMainLooper())
    private val listeners = mutableSetOf<(JSONObject) -> Unit>()
    private lateinit var audioManager: AudioManager
    private lateinit var session: MediaSession
    private lateinit var wakeLock: PowerManager.WakeLock
    private var engine: LibVLC? = null
    private var playback: MediaPlayer? = null
    private var media: Media? = null
    private var details: JSONObject? = null
    private val queue = NativePlaybackQueue()
    private var surface: VLCVideoLayout? = null
    private var videoOutputEnabled = false
    private var desiredPlaying = false
    private var resumeOnFocusGain = false
    private var hasFocus = false
    private var foreground = false
    private var closing = false
    private var startPosition = 0L
    private var lastPosition = 0L
    private var lastDuration = 0L
    private var errorMessage: String? = null
    private var sessionId = ""
    private var focusRequest: AudioFocusRequest? = null
    private var audioDelayMs = 0L
    private var softwareVideo = false
    private val videoCallback = object : IVLCVout.Callback {
        override fun onSurfacesCreated(output: IVLCVout) {
            val player = playback ?: return
            // VLC itself enables video during surface creation. Apply our display
            // choice afterwards, and refresh a paused still only once the surface exists.
            handler.post {
                if (!closing && playback === player && player.vlcVout === output) {
                    videoOutputEnabled = false
                    updateVideoOutput()
                }
            }
        }
        override fun onSurfacesDestroyed(output: IVLCVout) { videoOutputEnabled = false }
    }
    var audioSessionId = 0
        private set
    var backgroundAudio = false
        private set
    var displayMode = "video"
        private set
    var uiVisible = false
        private set

    private val focusListener = AudioManager.OnAudioFocusChangeListener { change ->
        when (change) {
            AudioManager.AUDIOFOCUS_GAIN -> {
                hasFocus = true
                if (resumeOnFocusGain && canPlay()) {
                    resumeOnFocusGain = false
                    play()
                }
            }
            AudioManager.AUDIOFOCUS_LOSS_TRANSIENT, AudioManager.AUDIOFOCUS_LOSS_TRANSIENT_CAN_DUCK -> {
                resumeOnFocusGain = desiredPlaying
                pause(abandon = false)
                hasFocus = false
            }
            AudioManager.AUDIOFOCUS_LOSS -> pause()
        }
    }
    private val noisyReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            if (intent.action == AudioManager.ACTION_AUDIO_BECOMING_NOISY) pause()
        }
    }
    private val ticker = object : Runnable {
        override fun run() {
            if (closing) return
            if (desiredPlaying) publish()
            handler.postDelayed(this, 1000)
        }
    }

    override fun onCreate() {
        super.onCreate()
        NativeBridge.playback = this
        audioManager = getSystemService(AudioManager::class.java)
        audioSessionId = audioManager.generateAudioSessionId()
        audioDelayMs = getSharedPreferences("native-playback", MODE_PRIVATE)
            .getLong("audio-delay-ms", 0L).coerceIn(-5000L, 5000L)
        wakeLock = getSystemService(PowerManager::class.java)
            .newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "$packageName:playback")
        if (Build.VERSION.SDK_INT >= 26) getSystemService(NotificationManager::class.java).createNotificationChannel(
            NotificationChannel(CHANNEL, "Media playback", NotificationManager.IMPORTANCE_LOW))
        session = MediaSession(this, "Lots of Videos").apply {
            // Still required for headset transport buttons on our Android 7 minimum.
            @Suppress("DEPRECATION")
            setFlags(MediaSession.FLAG_HANDLES_MEDIA_BUTTONS or MediaSession.FLAG_HANDLES_TRANSPORT_CONTROLS)
            setSessionActivity(playerIntent())
            setCallback(object : MediaSession.Callback() {
                override fun onPlay() = play()
                override fun onPause() = pause()
                override fun onStop() = stopPlayback()
                override fun onSeekTo(pos: Long) = seekTo(pos)
                override fun onRewind() = seek(-10)
                override fun onFastForward() = seek(30)
                override fun onSkipToNext() { skip(1) }
                override fun onSkipToPrevious() = previous()
                override fun onCustomAction(action: String, extras: android.os.Bundle?) {
                    when (action) { REWIND -> seek(-10); FORWARD -> seek(30); STOP -> stopPlayback() }
                }
            }, handler)
        }
        ContextCompat.registerReceiver(this, noisyReceiver,
            IntentFilter(AudioManager.ACTION_AUDIO_BECOMING_NOISY), ContextCompat.RECEIVER_NOT_EXPORTED)
        handler.post(ticker)
    }

    override fun onBind(intent: Intent): IBinder = LocalBinder()

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            PLAY -> play()
            PAUSE -> pause()
            REWIND -> seek(-10)
            FORWARD -> seek(30)
            NEXT -> skip(1)
            PREVIOUS -> previous()
            STOP -> stopPlayback()
        }
        // A process killed by Android must never unexpectedly restart audible media.
        return START_NOT_STICKY
    }

    fun open(request: JSONObject) {
        if (!request.has("items") && queue.playlistId.isEmpty() &&
            details?.optString("videoId") == request.optString("videoId") && playback != null && !closing) {
            publish()
            return
        }
        try {
            openItem(queue.load(request))
        } catch (error: Exception) {
            errorMessage = error.message ?: "This playlist could not be opened"
            stopPlayback()
        }
    }

    private fun openItem(request: JSONObject, continuation: Boolean = false) {
        val rate = if (continuation) playback?.rate ?: 1f else 1f
        val volume = if (continuation) playback?.volume ?: 100 else 100
        if (playback != null) pause()
        releaseDecoder()
        if (!continuation) {
            stopForeground(STOP_FOREGROUND_REMOVE)
            foreground = false
            sessionId = UUID.randomUUID().toString()
            backgroundAudio = false
            displayMode = "video"
        }
        closing = false
        details = request
        errorMessage = null
        lastPosition = 0
        lastDuration = 0
        startPosition = (request.optDouble("startTime", 0.0).coerceAtLeast(0.0) * 1000).toLong()
        try {
            check(audioSessionId > 0) { "Android did not allocate an audio session" }
            val vlc = LibVLC(applicationContext, arrayListOf("--audio-time-stretch", "--no-video-title-show",
                "--audiotrack-session-id=$audioSessionId"))
            engine = vlc
            val player = MediaPlayer(vlc)
            playback = player
            player.vlcVout.addCallback(videoCallback)
            // Decode every supported audio format to PCM so analysis sees this session's audio.
            // This retains VLC's demuxers/codecs, including MKV and E-AC-3.
            player.setAudioOutput("android_audiotrack")
            player.setAudioOutputDevice("pcm")
            surface?.let { player.attachViews(it, null, true, false) }
            var settingsPending = true
            player.setEventListener { event ->
                if (closing || playback !== player) return@setEventListener
                when (event.type) {
                    MediaPlayer.Event.Playing -> {
                        if (settingsPending) {
                            settingsPending = false
                            player.setRate(rate)
                            // VLC initializes its audio output asynchronously and resets
                            // volume then. Restore it after that output is actually ready.
                            player.setVolume(volume)
                            player.setAudioDelay(audioDelayMs * 1000L)
                        }
                        restoreStartPosition(player)
                        if (!desiredPlaying) player.pause()
                        updateVideoOutput()
                        publish()
                    }
                    MediaPlayer.Event.SeekableChanged, MediaPlayer.Event.LengthChanged -> restoreStartPosition(player)
                    MediaPlayer.Event.EncounteredError -> {
                        errorMessage = "This video could not be played."
                        pause()
                    }
                    MediaPlayer.Event.EndReached -> {
                        lastPosition = player.length.coerceAtLeast(lastPosition)
                        publish()
                        // Release/recreate VLC outside the end-event callback. The service
                        // advances even while its Activity and WebView are in the background.
                        handler.post {
                            if (!closing && playback === player && (!desiredPlaying || !skip(1))) stopPlayback()
                        }
                    }
                    MediaPlayer.Event.Paused -> publish()
                }
            }
            val path = request.getString("path")
            softwareVideo = NativeVideoDecoder.requiresSoftware(path)
            media = Media(vlc, Uri.fromFile(File(path))).also {
                it.setHWDecoderEnabled(!softwareVideo, false)
                player.media = it
            }
            session.isActive = true
            play()
        } catch (error: Exception) {
            errorMessage = error.message ?: "Native player initialization failed"
            stopPlayback()
        }
    }

    fun addListener(listener: (JSONObject) -> Unit) { listeners.add(listener); listener(snapshot()) }
    fun removeListener(listener: (JSONObject) -> Unit) { listeners.remove(listener) }

    private fun restoreStartPosition(player: MediaPlayer) {
        val duration = player.length
        if (startPosition <= 0 || !player.isSeekable || duration <= 0) return
        val position = startPosition
        startPosition = 0
        // A saved EOF can come from an older app or a slightly different
        // metadata duration. Leave completed items at the beginning.
        if (position < duration - minOf(1000L, duration / 50)) player.setTime(position, false)
    }

    fun attachSurface(view: VLCVideoLayout) {
        if (surface === view) return
        detachSurface()
        surface = view
        videoOutputEnabled = false
        playback?.attachViews(view, null, true, false)
        updateVideoOutput()
    }

    fun detachSurface(expected: VLCVideoLayout? = null) {
        if (expected != null && surface !== expected) return
        if (surface == null) return
        playback?.setVideoTrackEnabled(false)
        videoOutputEnabled = false
        playback?.detachViews()
        surface = null
    }

    fun setUiVisible(visible: Boolean) {
        uiVisible = visible
        if (!visible && !backgroundAudio) pause() else publish()
        updateVideoOutput()
    }

    fun setBackgroundAudio(enabled: Boolean) {
        if (closing || playback == null) return
        backgroundAudio = enabled
        if (enabled) {
            // Called by the visible player, before it can go into the background.
            startService(Intent(this, NativePlaybackService::class.java))
            promoteToForeground()
        } else {
            if (!uiVisible) pause()
            stopForeground(STOP_FOREGROUND_REMOVE)
            foreground = false
        }
        publish()
    }

    fun setDisplayMode(mode: String) {
        if (mode != "video" && mode != "visualizer") return
        displayMode = mode
        updateVideoOutput()
        publish()
    }

    private fun updateVideoOutput() {
        val player = playback ?: return
        val enabled = uiVisible && surface != null && displayMode == "video"
        val refreshPausedFrame = enabled && !videoOutputEnabled && !desiredPlaying && player.isSeekable
        val position = if (desiredPlaying) player.time.coerceAtLeast(0) else lastPosition
        player.setVideoTrackEnabled(enabled)
        videoOutputEnabled = enabled
        // Enabling VLC's video track while paused creates an empty output. A precise
        // seek at the same timestamp decodes its still frame without playing audio.
        if (refreshPausedFrame) player.setTime(position, false)
    }

    private fun canPlay() = playback != null && !closing && (uiVisible || backgroundAudio)

    fun play() {
        if (!canPlay()) return
        if (backgroundAudio) promoteToForeground()
        if (!hasFocus) {
            val result = if (Build.VERSION.SDK_INT >= 26) {
                val request = focusRequest ?: AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN)
                    .setAudioAttributes(AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_MEDIA)
                        .setContentType(AudioAttributes.CONTENT_TYPE_MOVIE).build())
                    .setWillPauseWhenDucked(true).setOnAudioFocusChangeListener(focusListener, handler)
                    .build().also { focusRequest = it }
                audioManager.requestAudioFocus(request)
            } else {
                @Suppress("DEPRECATION")
                audioManager.requestAudioFocus(focusListener, AudioManager.STREAM_MUSIC, AudioManager.AUDIOFOCUS_GAIN)
            }
            hasFocus = result == AudioManager.AUDIOFOCUS_REQUEST_GRANTED
        }
        if (!hasFocus) {
            desiredPlaying = false
            publish()
            return
        }
        resumeOnFocusGain = false
        // Decoding a paused still can advance VLC's internal preroll clock.
        // Resume from the held user position rather than that decoder timestamp.
        // An explicit seek can already be pending. Submitting it again while
        // resuming can stall VLC's decoder after an audio-track change.
        val resumePosition = lastPosition.takeIf {
            !desiredPlaying && playback?.isSeekable == true && playback?.time != lastPosition
        }
        desiredPlaying = true
        playback?.play()
        if (resumePosition != null) playback?.setTime(resumePosition, false)
        publish()
    }

    fun pause(abandon: Boolean = true) {
        if (desiredPlaying) lastPosition = playback?.time?.takeIf { it >= 0 } ?: lastPosition
        desiredPlaying = false
        playback?.pause()
        if (abandon) abandonFocus()
        publish()
    }

    private fun abandonFocus() {
        resumeOnFocusGain = false
        if (Build.VERSION.SDK_INT >= 26) focusRequest?.let { audioManager.abandonAudioFocusRequest(it) }
        else {
            @Suppress("DEPRECATION")
            audioManager.abandonAudioFocus(focusListener)
        }
        hasFocus = false
    }

    fun seek(seconds: Int) = seekTo((if (desiredPlaying) playback?.time ?: lastPosition else lastPosition) + seconds * 1000L)

    fun seekTo(position: Long) {
        playback?.let {
            // A paused fast seek can settle at a later keyframe after the UI has
            // already shown its requested time. Keep paused stills at the exact time.
            if (it.isSeekable) {
                lastPosition = position.coerceIn(0, it.length.coerceAtLeast(0))
                it.setTime(lastPosition, desiredPlaying)
            }
        }
        publish()
    }

    fun tracks(subtitles: Boolean): Array<MediaPlayer.TrackDescription> =
        (if (subtitles) playback?.spuTracks else playback?.audioTracks) ?: emptyArray()

    fun setAudioDelay(milliseconds: Long): Boolean {
        val delay = milliseconds.coerceIn(-5000L, 5000L)
        // LibVLC uses microseconds; a negative value advances late audio.
        if (playback?.setAudioDelay(delay * 1000L) != true) return false
        audioDelayMs = delay
        getSharedPreferences("native-playback", MODE_PRIVATE).edit()
            .putLong("audio-delay-ms", delay).apply()
        publish()
        return true
    }

    fun control(request: JSONObject) {
        when (request.optString("action")) {
            "seek" -> seek(request.optInt("seconds"))
            "seekTo" -> seekTo((request.optDouble("position") * 1000).toLong())
            "togglePause" -> if (desiredPlaying) pause() else play()
            "play" -> play()
            "pause" -> pause()
            "stop" -> stopPlayback()
            "next" -> skip(1)
            "previous" -> previous()
            "subtitle" -> playback?.setSpuTrack(request.optInt("track", -1))
            "audio" -> playback?.setAudioTrack(request.optInt("track", -1))
            "audioDelay" -> setAudioDelay(request.optLong("milliseconds"))
            "rate" -> playback?.setRate(request.optDouble("rate", 1.0).toFloat().coerceIn(0.25f, 4f))
            "volume" -> playback?.setVolume(request.optInt("volume", 100).coerceIn(0, 100))
        }
        publish()
    }

    private fun skip(direction: Int): Boolean {
        if (closing) return false
        val originalIndex = queue.index
        while (true) {
            val next = queue.move(direction) ?: run {
                queue.move(originalIndex - queue.index)
                publish()
                return false
            }
            // A file can be removed after the queue was opened. Continue to the
            // next available entry without re-entering the WebView or restarting it.
            if (!File(next.getString("path")).isFile) continue
            openItem(next, continuation = true)
            return true
        }
    }

    private fun previous() { if (!skip(-1)) seekTo(0) }

    fun snapshot(): JSONObject {
        val player = playback
        if (!closing && player != null) {
            // While paused, the user timeline stays fixed even when VLC decodes
            // preroll to restore a video surface or change its output tracks.
            if (desiredPlaying) lastPosition = player.time.takeIf { it >= 0 } ?: lastPosition
            lastDuration = player.length.takeIf { it > 0 } ?: lastDuration
        }
        val stats = media?.stats
        return JSONObject().put("videoId", details?.optString("videoId") ?: "")
            .put("video", details?.optString("video") ?: "")
            .put("collection", details?.optString("collection") ?: "")
            .put("currentSrc", details?.optString("path") ?: "")
            .put("currentTime", lastPosition / 1000.0).put("duration", lastDuration / 1000.0)
            .put("paused", !desiredPlaying || closing).put("playing", player?.isPlaying == true && !closing)
            .put("playbackRate", player?.rate?.toDouble() ?: 1.0).put("volume", player?.volume ?: 100)
            .put("audioDelayMs", (player?.audioDelay ?: audioDelayMs * 1000L) / 1000L)
            .put("videoDecoder", if (softwareVideo) "software" else "auto")
            .put("currentSubtitleTrack", player?.spuTrack ?: -1).put("currentAudioTrack", player?.audioTrack ?: -1)
            .put("audioTracks", player?.audioTracksCount ?: 0).put("decodedAudio", stats?.decodedAudio ?: 0)
            .put("decodedVideo", stats?.decodedVideo ?: 0).put("backgroundAudio", backgroundAudio)
            .put("displayMode", displayMode).put("sessionId", sessionId).put("audioSessionId", audioSessionId)
            .put("playlistId", queue.playlistId).put("playlistName", queue.playlistName)
            .put("queueIndex", queue.index).put("queueLength", queue.size).put("shuffle", queue.shuffled)
            .put("hasPrevious", queue.hasPrevious).put("hasNext", queue.hasNext)
            .put("closed", closing).put("error", errorMessage ?: JSONObject.NULL)
    }

    private fun publish() {
        if (details == null) return
        val state = snapshot()
        NativeBridge.publish(state)
        if (::session.isInitialized) updateSession(state)
        if (::wakeLock.isInitialized) {
            val keepAwake = backgroundAudio && desiredPlaying && !closing
            if (keepAwake && !wakeLock.isHeld) wakeLock.acquire()
            else if (!keepAwake && wakeLock.isHeld) wakeLock.release()
        }
        listeners.toList().forEach { it(state) }
    }

    private fun updateSession(state: JSONObject) {
        session.setMetadata(MediaMetadata.Builder()
            .putString(MediaMetadata.METADATA_KEY_TITLE, state.optString("video"))
            .putString(MediaMetadata.METADATA_KEY_ALBUM, state.optString("collection"))
            .putLong(MediaMetadata.METADATA_KEY_DURATION, lastDuration).build())
        val status = if (closing) PlaybackState.STATE_STOPPED else if (desiredPlaying) {
            if (playback?.isPlaying == true) PlaybackState.STATE_PLAYING else PlaybackState.STATE_BUFFERING
        } else PlaybackState.STATE_PAUSED
        session.setPlaybackState(PlaybackState.Builder()
            .setActions(PlaybackState.ACTION_PLAY or PlaybackState.ACTION_PAUSE or PlaybackState.ACTION_PLAY_PAUSE or
                PlaybackState.ACTION_STOP or PlaybackState.ACTION_SEEK_TO or PlaybackState.ACTION_REWIND or PlaybackState.ACTION_FAST_FORWARD or
                (if (queue.hasPrevious) PlaybackState.ACTION_SKIP_TO_PREVIOUS else 0L) or
                (if (queue.hasNext) PlaybackState.ACTION_SKIP_TO_NEXT else 0L))
            .setState(status, lastPosition, if (desiredPlaying) playback?.rate ?: 1f else 0f)
            .addCustomAction(REWIND, "Back 10 seconds", android.R.drawable.ic_media_rew)
            .addCustomAction(FORWARD, "Forward 30 seconds", android.R.drawable.ic_media_ff)
            .addCustomAction(STOP, "Stop playback", android.R.drawable.ic_menu_close_clear_cancel).build())
        if (foreground && !closing) getSystemService(NotificationManager::class.java).notify(NOTIFICATION, notification())
    }

    private fun playerIntent(): PendingIntent = PendingIntent.getActivity(this, 0,
        Intent(this, NativePlayerActivity::class.java)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP),
        PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)

    private fun actionIntent(action: String) = PendingIntent.getService(this, action.hashCode(),
        Intent(this, NativePlaybackService::class.java).setAction(action),
        PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)

    private fun notification(): Notification {
        val builder = if (Build.VERSION.SDK_INT >= 26) Notification.Builder(this, CHANNEL) else {
            @Suppress("DEPRECATION")
            Notification.Builder(this)
        }
        return builder.setSmallIcon(android.R.drawable.ic_media_play)
            .setContentTitle(details?.optString("video"))
            .setContentText(if (desiredPlaying) "Audio continues with screen off" else "Paused")
            .setContentIntent(playerIntent()).setDeleteIntent(actionIntent(STOP))
            .setVisibility(Notification.VISIBILITY_PUBLIC).setOnlyAlertOnce(true)
            .setOngoing(desiredPlaying).setCategory(Notification.CATEGORY_TRANSPORT)
            .addAction(Notification.Action.Builder(
                if (queue.playlistId.isNotEmpty()) android.R.drawable.ic_media_previous else android.R.drawable.ic_media_rew,
                if (queue.playlistId.isNotEmpty()) "Previous" else "−10 s",
                actionIntent(if (queue.playlistId.isNotEmpty()) PREVIOUS else REWIND)).build())
            .addAction(Notification.Action.Builder(if (desiredPlaying) android.R.drawable.ic_media_pause else android.R.drawable.ic_media_play,
                if (desiredPlaying) "Pause" else "Play", actionIntent(if (desiredPlaying) PAUSE else PLAY)).build())
            .addAction(Notification.Action.Builder(
                if (queue.playlistId.isNotEmpty()) android.R.drawable.ic_media_next else android.R.drawable.ic_media_ff,
                if (queue.playlistId.isNotEmpty()) "Next" else "+30 s",
                actionIntent(if (queue.playlistId.isNotEmpty()) NEXT else FORWARD)).build())
            .addAction(Notification.Action.Builder(android.R.drawable.ic_menu_close_clear_cancel, "Stop", actionIntent(STOP)).build())
            .setStyle(Notification.MediaStyle().setMediaSession(session.sessionToken).setShowActionsInCompactView(0, 1, 2))
            .build()
    }

    private fun promoteToForeground() {
        if (foreground || closing) return
        if (Build.VERSION.SDK_INT >= 29) startForeground(NOTIFICATION, notification(), ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK)
        else startForeground(NOTIFICATION, notification())
        foreground = true
    }

    fun stopPlayback() {
        if (closing) return
        snapshot() // Retain the last valid position before VLC resets its clock.
        closing = true
        desiredPlaying = false
        abandonFocus()
        publish()
        session.isActive = false
        stopForeground(STOP_FOREGROUND_REMOVE)
        foreground = false
        releaseDecoder()
        stopSelf()
    }

    private fun releaseDecoder() {
        if (playback == null && engine == null) return
        abandonFocus()
        playback?.run { vlcVout.removeCallback(videoCallback); setEventListener(null); stop(); detachViews(); release() }
        playback = null
        videoOutputEnabled = false
        media?.release()
        media = null
        engine?.release()
        engine = null
    }

    override fun onTaskRemoved(rootIntent: Intent?) {
        if (!backgroundAudio || !desiredPlaying) stopPlayback()
        super.onTaskRemoved(rootIntent)
    }

    override fun onDestroy() {
        if (!closing) stopPlayback()
        handler.removeCallbacksAndMessages(null)
        unregisterReceiver(noisyReceiver)
        if (wakeLock.isHeld) wakeLock.release()
        session.release()
        listeners.clear()
        surface = null
        if (NativeBridge.playback === this) NativeBridge.playback = null
        super.onDestroy()
    }

    companion object {
        private const val CHANNEL = "native-playback"
        const val NOTIFICATION = 41
        const val PLAY = "com.abamaxa.lots_of_videos.PLAY"
        const val PAUSE = "com.abamaxa.lots_of_videos.PAUSE"
        const val REWIND = "com.abamaxa.lots_of_videos.REWIND"
        const val FORWARD = "com.abamaxa.lots_of_videos.FORWARD"
        const val NEXT = "com.abamaxa.lots_of_videos.NEXT"
        const val PREVIOUS = "com.abamaxa.lots_of_videos.PREVIOUS"
        const val STOP = "com.abamaxa.lots_of_videos.STOP"
    }
}
