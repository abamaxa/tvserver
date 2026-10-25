package com.abamaxa.lots_of_videos

import android.annotation.SuppressLint
import android.content.Context
import android.graphics.Color
import android.os.Handler
import android.os.Looper
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.webkit.WebViewAssetLoader
import java.io.ByteArrayInputStream

/** Sandboxed local renderer. Audio travels into JS as analysis bytes, never as another player. */
@SuppressLint("SetJavaScriptEnabled")
class NativeVisualizerView(context: Context, private val onError: (String) -> Unit) : WebView(context) {
    private val handler = Handler(Looper.getMainLooper())
    private var ready = false
    private var active = false
    private var sessionId = 0
    private var analysis: NativeAudioAnalysis? = null
    val analysisFrames get() = analysis?.frames ?: 0
    val analysisRms get() = analysis?.rms ?: 0.0
    private val capture = object : Runnable {
        override fun run() {
            if (!active || !ready) return
            try {
                val source = analysis ?: NativeAudioAnalysis(sessionId).also { analysis = it }
                val frame = source.frame() ?: error("Android stopped providing playback analysis")
                evaluateJavascript(frame, null)
                handler.postDelayed(this, 50)
            } catch (error: Exception) {
                setPlayback(false, sessionId)
                onError(error.message ?: "Audio analysis is unavailable on this device")
            }
        }
    }

    init {
        setBackgroundColor(Color.BLACK)
        contentDescription = "Classic audio visualizer"
        settings.javaScriptEnabled = true
        settings.allowFileAccess = false
        settings.allowContentAccess = false
        settings.domStorageEnabled = false
        settings.mediaPlaybackRequiresUserGesture = true
        val assets = WebViewAssetLoader.Builder()
            .addPathHandler("/assets/", WebViewAssetLoader.AssetsPathHandler(context)).build()
        webViewClient = object : WebViewClient() {
            override fun shouldInterceptRequest(view: WebView, request: WebResourceRequest): WebResourceResponse =
                assets.shouldInterceptRequest(request.url)
                    ?: WebResourceResponse("text/plain", "utf-8", ByteArrayInputStream(ByteArray(0)))
            override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest) = true
            override fun onPageFinished(view: WebView, url: String) {
                evaluateJavascript("Boolean(window.nativeVisualizer)") { initialized ->
                    if (initialized != "true") {
                        setPlayback(false, sessionId)
                        onError("The bundled renderer could not start")
                    } else {
                        ready = true
                        if (active) handler.post(capture)
                    }
                }
            }
        }
        loadUrl("https://appassets.androidplatform.net/assets/visualizer/index.html")
    }

    fun setPlayback(playing: Boolean, audioSession: Int) {
        if (active == playing && sessionId == audioSession) return
        active = playing
        sessionId = audioSession
        handler.removeCallbacks(capture)
        if (playing && ready) handler.post(capture)
        else if (!playing) {
            evaluateJavascript("window.nativeVisualizer?.pause()", null)
            analysis?.close()
            analysis = null
        }
    }

    override fun destroy() {
        setPlayback(false, sessionId)
        handler.removeCallbacksAndMessages(null)
        super.destroy()
    }
}
