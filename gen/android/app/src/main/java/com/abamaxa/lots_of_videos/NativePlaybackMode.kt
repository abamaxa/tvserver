package com.abamaxa.lots_of_videos

import org.json.JSONObject

/** Listening intent belongs to the session, independently of its video/visualizer display. */
internal enum class NativePlaybackMode(val value: String) {
    WATCH("watch"), LISTEN("listen");

    companion object {
        fun from(value: String): NativePlaybackMode? = entries.firstOrNull { it.value == value }
        fun defaultFor(item: JSONObject) = if (item.optBoolean("audioOnly")) LISTEN else WATCH
    }
}
