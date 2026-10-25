package com.abamaxa.lots_of_videos

import android.media.audiofx.Visualizer
import android.util.Base64
import kotlin.math.sqrt

/** Reads only VLC's allocated output session. Never records a microphone or the global mix. */
class NativeAudioAnalysis(sessionId: Int) : AutoCloseable {
    private val effect: Visualizer
    private val waveform = ByteArray(1024)
    private val fft = ByteArray(1024)
    var frames = 0
        private set
    var rms = 0.0
        private set

    init {
        require(sessionId > 0) { "A dedicated playback audio session is required" }
        effect = Visualizer(sessionId)
        try {
            check(effect.setCaptureSize(1024) == Visualizer.SUCCESS) { "Audio analysis capture size is unsupported" }
            check(effect.setScalingMode(Visualizer.SCALING_MODE_AS_PLAYED) == Visualizer.SUCCESS)
            check(effect.setEnabled(true) == Visualizer.SUCCESS) { "Audio analysis could not start" }
        } catch (error: Exception) {
            effect.release()
            throw error
        }
    }

    fun frame(): String? {
        if (effect.getWaveForm(waveform) != Visualizer.SUCCESS || effect.getFft(fft) != Visualizer.SUCCESS) return null
        var sum = 0.0
        for (sample in waveform) {
            val value = ((sample.toInt() and 255) - 128) / 128.0
            sum += value * value
        }
        rms = sqrt(sum / waveform.size)
        frames++
        return "window.nativeVisualizer?.frame('${Base64.encodeToString(waveform, Base64.NO_WRAP)}'," +
            "'${Base64.encodeToString(fft, Base64.NO_WRAP)}',${effect.samplingRate / 1000.0})"
    }

    override fun close() { effect.enabled = false; effect.release() }
}
