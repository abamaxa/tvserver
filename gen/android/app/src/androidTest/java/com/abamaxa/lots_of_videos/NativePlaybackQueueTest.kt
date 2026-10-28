package com.abamaxa.lots_of_videos

import androidx.test.ext.junit.runners.AndroidJUnit4
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class NativePlaybackQueueTest {
    private fun item(id: String) = JSONObject().put("videoId", id).put("path", "/cache/$id.mp3")
    private fun request(shuffle: Boolean = false) = JSONObject()
        .put("playlistId", "mix").put("playlistName", "Evening mix").put("shuffle", shuffle)
        .put("items", JSONArray(listOf(item(Long.MIN_VALUE.toString()), item("2"), item(Long.MAX_VALUE.toString()))))

    @Test fun keepsOrderSelectionAndBoundaries() {
        val queue = NativePlaybackQueue()
        assertEquals("2", queue.load(request().put("startVideoId", "2")).getString("videoId"))
        assertEquals(1, queue.index)
        assertTrue(queue.hasPrevious)
        assertTrue(queue.hasNext)
        assertEquals(Long.MAX_VALUE.toString(), queue.move(1)!!.getString("videoId"))
        assertFalse(queue.hasNext)
        assertNull(queue.move(1))
        assertEquals("2", queue.move(-1)!!.getString("videoId"))
        assertEquals(Long.MIN_VALUE.toString(), queue.move(-1)!!.getString("videoId"))
        assertFalse(queue.hasPrevious)
        assertNull(queue.move(-1))
    }

    @Test fun shuffleVisitsEveryEntryOnceAndPreviousRetracesThatOrder() {
        val queue = NativePlaybackQueue()
        val first = queue.load(request(true).put("startVideoId", "2"))
        assertEquals("2", first.getString("videoId"))
        assertTrue(queue.shuffled)
        val order = mutableListOf(first.getString("videoId"))
        while (queue.hasNext) order.add(queue.move(1)!!.getString("videoId"))
        assertEquals(setOf(Long.MIN_VALUE.toString(), "2", Long.MAX_VALUE.toString()), order.toSet())
        assertEquals(3, order.size)
        assertEquals(order[1], queue.move(-1)!!.getString("videoId"))
        assertEquals(order[0], queue.move(-1)!!.getString("videoId"))
    }

    @Test fun singleItemSelectionReplacesThePlaylistAndInvalidSelectionsAreRejected() {
        val queue = NativePlaybackQueue()
        queue.load(request())
        queue.load(item("single"))
        assertEquals(1, queue.size)
        assertEquals("", queue.playlistId)
        assertFalse(queue.shuffled)
        assertNull(queue.move(1))
        try {
            queue.load(request().put("startVideoId", "missing"))
            fail("Missing selection must be rejected")
        } catch (_: IllegalArgumentException) {}
        try {
            queue.load(JSONObject().put("items", JSONArray()))
            fail("Empty playlist must be rejected")
        } catch (_: IllegalArgumentException) {}
    }

    @Test fun appendingPreservesTheCurrentItemAndTheRemainingShuffleOrder() {
        val queue = NativePlaybackQueue()
        queue.load(request(true).put("startVideoId", "2"))
        val remaining = mutableListOf<String>()
        while (queue.hasNext) remaining.add(queue.move(1)!!.getString("videoId"))
        queue.move(-queue.index)
        queue.append(item("new"))
        assertEquals("mix", queue.playlistId)
        assertEquals(0, queue.index)
        assertEquals(4, queue.size)
        for (id in remaining) assertEquals(id, queue.move(1)!!.getString("videoId"))
        assertEquals("new", queue.move(1)!!.getString("videoId"))
        assertFalse(queue.hasNext)
    }
}
