package com.abamaxa.lots_of_videos

import org.json.JSONObject

/** An in-memory playback snapshot. Saved playlist membership stays in SQLite. */
internal class NativePlaybackQueue {
    private var items = emptyList<JSONObject>()
    var index = 0
        private set
    var playlistId = ""
        private set
    var playlistName = ""
        private set
    var shuffled = false
        private set
    val size get() = items.size
    val hasPrevious get() = index > 0
    val hasNext get() = index + 1 < size

    fun load(request: JSONObject): JSONObject {
        val array = request.optJSONArray("items")
        val entries = if (array == null) listOf(request) else
            (0 until array.length()).map { array.getJSONObject(it) }
        require(entries.isNotEmpty()) { "This playlist has no items to play" }
        require(entries.all { it.optString("videoId").isNotEmpty() && it.optString("path").isNotEmpty() }) {
            "This playlist contains an invalid item"
        }
        val selected = request.optString("startVideoId", "").takeUnless { it.isEmpty() || it == "null" }
        require(selected == null || entries.any { it.getString("videoId") == selected }) { "Playlist item unavailable" }
        shuffled = request.optBoolean("shuffle")
        items = if (shuffled) entries.shuffled() else entries
        // Shuffle visits every item once, starting with the explicitly selected item if given.
        if (shuffled && selected != null) items = items.sortedBy { if (it.getString("videoId") == selected) 0 else 1 }
        index = if (selected == null) 0 else items.indexOfFirst { it.getString("videoId") == selected }
        playlistId = request.optString("playlistId", "")
        playlistName = request.optString("playlistName", "")
        return items[index]
    }

    fun move(direction: Int): JSONObject? {
        val next = index + direction
        if (next !in items.indices) return null
        index = next
        return items[index]
    }
}
