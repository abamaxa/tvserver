package com.abamaxa.lots_of_videos

/** Difference between mixer consumption and the output's presentation clock, in microseconds. */
internal object NativeAudioLatencyEstimate {
    fun sample(headFrames: Long, presentedFrames: Long, timestampNanos: Long, nowNanos: Long, rate: Int): Long? {
        if (rate <= 0 || headFrames < 0 || presentedFrames < 0 || timestampNanos <= 0) return null
        val age = nowNanos - timestampNanos
        // AudioTimestamp may describe frames committed for future presentation.
        if (age !in -1_000_000_000L..2_000_000_000L) return null
        val presentedNow = presentedFrames.toDouble() + age.toDouble() * rate / 1_000_000_000.0
        val delay = ((headFrames - presentedNow) * 1_000_000.0 / rate).toLong()
        return delay.takeIf { it in 0..2_000_000L }
    }

    fun stable(samples: List<Long>): Long? {
        if (samples.size < 3) return null
        val ordered = samples.takeLast(5).sorted()
        if (ordered.last() - ordered.first() > 40_000L) return null
        return ordered[ordered.size / 2]
    }

    fun effective(manualMs: Long, outputLatencyUs: Long) = manualMs.coerceIn(-5000, 5000) * 1000 - outputLatencyUs
}
