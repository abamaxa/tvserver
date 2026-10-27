package com.abamaxa.lots_of_videos

import android.media.MediaExtractor
import android.media.MediaFormat

/** Avoid the AV1 audio/video sync errors observed with VLC's MediaCodec decoder. */
internal object NativeVideoDecoder {
    fun requiresSoftware(path: String): Boolean {
        val extractor = MediaExtractor()
        return try {
            extractor.setDataSource(path)
            (0 until extractor.trackCount).any {
                extractor.getTrackFormat(it).getString(MediaFormat.KEY_MIME) == "video/av01"
            }
        } catch (_: Exception) {
            // Android's extractor understands fewer containers than VLC. Leave
            // unknown files to VLC's usual hardware/software codec fallback.
            false
        } finally {
            extractor.release()
        }
    }
}
