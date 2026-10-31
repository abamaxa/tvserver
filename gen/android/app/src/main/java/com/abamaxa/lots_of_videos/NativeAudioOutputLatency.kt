package com.abamaxa.lots_of_videos

import android.content.Context
import android.media.AudioAttributes
import android.media.AudioDeviceCallback
import android.media.AudioDeviceInfo
import android.media.AudioFormat
import android.media.AudioManager
import android.media.AudioTimestamp
import android.media.AudioTrack
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import kotlin.concurrent.thread

/** Measures the selected PCM output, without recording audio or persisting device-specific offsets. */
internal class NativeAudioOutputLatency(context: Context, private val changed: (Long) -> Unit) : AutoCloseable {
    private val audio = context.getSystemService(AudioManager::class.java)
    private val handler = Handler(Looper.getMainLooper())
    @Volatile private var generation = 0
    private var closed = false
    private var measuring = false
    private var route: Int? = null
    private var routes: Set<Int>? = null
    private var latency = 0L
    private val attributes = AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_MEDIA)
        .setContentType(AudioAttributes.CONTENT_TYPE_UNKNOWN).build()
    private val devices = object : AudioDeviceCallback() {
        override fun onAudioDevicesAdded(added: Array<AudioDeviceInfo>) {
            if (added.any { it.isSink }) { invalidateProbe(); checkRoute(force = true) }
        }
        override fun onAudioDevicesRemoved(removed: Array<AudioDeviceInfo>) {
            if (removed.any { it.id == route }) { route = null; report(0) }
            if (removed.any { it.isSink }) { invalidateProbe(); checkRoute(force = true) }
        }
    }

    init { audio.registerAudioDeviceCallback(devices, handler) }

    /** Called by the service ticker as well as playback, so an already-connected output can change. */
    fun checkRoute(force: Boolean = false) {
        if (closed) return
        val selected = if (Build.VERSION.SDK_INT >= 33) runCatching {
            audio.getAudioDevicesForAttributes(attributes).map { it.id }.toSet()
        }.getOrNull() else null
        if (selected != null && selected != routes) {
            routes = selected
            route = null
            report(0)
            measure()
        } else if (force && !measuring && (selected == null || route == null)) measure()
    }

    private fun report(value: Long) {
        if (latency == value) return
        latency = value
        changed(value)
    }

    private fun invalidateProbe() {
        if (measuring) { generation++; measuring = false }
    }

    private fun measure() {
        val token = ++generation
        measuring = true
        thread(name = "playback-output-timing", isDaemon = true) {
            var track: AudioTrack? = null
            var output: Int? = null
            var result = 0L
            try {
                val rate = 44100
                val size = maxOf(16384, AudioTrack.getMinBufferSize(rate,
                    AudioFormat.CHANNEL_OUT_STEREO, AudioFormat.ENCODING_PCM_16BIT))
                @Suppress("DEPRECATION")
                val probe = AudioTrack(AudioManager.STREAM_MUSIC, rate, AudioFormat.CHANNEL_OUT_STEREO,
                    AudioFormat.ENCODING_PCM_16BIT, size, AudioTrack.MODE_STREAM)
                track = probe
                check(probe.state == AudioTrack.STATE_INITIALIZED)
                val timestamp = AudioTimestamp()
                val silence = ByteArray(2048)
                val samples = mutableListOf<Long>()
                var lastSample = 0L
                val deadline = SystemClock.elapsedRealtime() + 4000
                probe.play()
                while (token == generation && SystemClock.elapsedRealtime() < deadline) {
                    if (probe.write(silence, 0, silence.size, AudioTrack.WRITE_NON_BLOCKING) < 0) break
                    val device = probe.routedDevice
                    if (device != null) {
                        if (output != null && output != device.id) samples.clear()
                        output = device.id
                        val bluetooth = device.type == AudioDeviceInfo.TYPE_BLUETOOTH_A2DP ||
                            device.type == AudioDeviceInfo.TYPE_HEARING_AID ||
                            device.type == AudioDeviceInfo.TYPE_BLE_HEADSET || device.type == AudioDeviceInfo.TYPE_BLE_SPEAKER
                        if (!bluetooth) break
                        val now = System.nanoTime()
                        if (now - lastSample >= 500_000_000L) {
                            lastSample = now
                            if (probe.getTimestamp(timestamp)) {
                                NativeAudioLatencyEstimate.sample(probe.playbackHeadPosition.toLong() and 0xffffffffL,
                                    timestamp.framePosition, timestamp.nanoTime, System.nanoTime(), rate)?.let { samples.add(it) }
                                val estimate = NativeAudioLatencyEstimate.stable(samples)
                                if (estimate != null) { result = estimate; break }
                            }
                        }
                    }
                    SystemClock.sleep(10)
                }
            } catch (_: Exception) {
                // Unsupported or unstable timestamps retain the user's manual correction.
            } finally {
                track?.let { runCatching { it.stop() }; it.release() }
            }
            handler.post {
                if (!closed && token == generation) {
                    measuring = false
                    route = output
                    report(result)
                }
            }
        }
    }

    override fun close() {
        closed = true
        generation++
        audio.unregisterAudioDeviceCallback(devices)
        handler.removeCallbacksAndMessages(null)
    }
}
