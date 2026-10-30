package com.abamaxa.lots_of_videos

import android.content.Context
import org.json.JSONObject
import java.util.Locale

internal data class NativePlaybackDefaults(
    val rate: Float = 1f,
    val audioDelayMs: Long = 0,
    val audioLanguage: String = "",
    val subtitleLanguage: String = "",
    val subtitles: Boolean = true,
    val videoMode: NativePlaybackMode = NativePlaybackMode.WATCH,
    val audioMode: NativePlaybackMode = NativePlaybackMode.LISTEN,
    val displayMode: String = "video",
) {
    fun modeFor(item: JSONObject) = if (item.optBoolean("audioOnly")) audioMode else videoMode

    fun toJson() = JSONObject().put("rate", rate.toDouble()).put("audioDelayMs", audioDelayMs)
        .put("audioLanguage", audioLanguage).put("subtitleLanguage", subtitleLanguage)
        .put("subtitles", subtitles).put("videoMode", videoMode.value).put("audioMode", audioMode.value)
        .put("displayMode", displayMode)

    companion object {
        const val STORE = "playback-defaults"
        fun load(context: Context): NativePlaybackDefaults {
            val saved = NativeSettingsStore.read(context, STORE)
            val legacyDelay = context.getSharedPreferences("native-playback", Context.MODE_PRIVATE)
                .getLong("audio-delay-ms", 0)
            return fromJson(saved, legacyDelay)
        }

        fun fromJson(saved: JSONObject, legacyDelay: Long = 0) = NativePlaybackDefaults(
            rate = saved.optDouble("rate", 1.0).takeIf { it.isFinite() && it in 0.25..4.0 }?.toFloat() ?: 1f,
            audioDelayMs = saved.optLong("audioDelayMs", legacyDelay).coerceIn(-5000, 5000),
            audioLanguage = validLanguage(saved.optString("audioLanguage", "")),
            subtitleLanguage = validLanguage(saved.optString("subtitleLanguage", "")),
            subtitles = saved.optBoolean("subtitles", true),
            videoMode = NativePlaybackMode.from(saved.optString("videoMode")) ?: NativePlaybackMode.WATCH,
            audioMode = NativePlaybackMode.from(saved.optString("audioMode")) ?: NativePlaybackMode.LISTEN,
            displayMode = saved.optString("displayMode").takeIf { it == "video" || it == "visualizer" } ?: "video",
        )

        private fun validLanguage(value: String) = value.trim().takeIf { runCatching { vlcLanguage(it) }.isSuccess } ?: ""

        /** Accept ISO 639-1/639-2 codes and BCP 47 tags; VLC expects ISO 639-2. */
        fun vlcLanguage(value: String): String? {
            if (value.isBlank()) return null
            val language = value.trim().lowercase(Locale.ROOT).replace('_', '-').substringBefore('-')
            require(language.matches(Regex("[a-z]{2,3}"))) { "Use a language code such as en, eng, es or spa" }
            if (language.length == 3) return language
            return try { Locale.forLanguageTag(language).isO3Language.takeIf { it.isNotEmpty() } ?: error("Unknown language code") }
            catch (_: java.util.MissingResourceException) { error("Unknown language code: $value") }
        }
    }
}
