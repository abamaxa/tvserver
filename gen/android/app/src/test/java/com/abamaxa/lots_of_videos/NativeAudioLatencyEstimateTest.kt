package com.abamaxa.lots_of_videos

import org.junit.Assert.*
import org.junit.Test

class NativeAudioLatencyEstimateTest {
    @Test fun bluetoothMixerConsumptionIsNotAudiblePresentation() {
        // Android has consumed 44,100 frames, but only 29,547 have reached the output.
        // The timestamp was sampled 2 ms ago: account for those extra 88.2 frames.
        assertEquals(328000L, NativeAudioLatencyEstimate.sample(44100, 29547, 998000000, 1000000000, 44100))
    }
    @Test fun timestampsCanCommitFramesForFuturePresentation() {
        assertEquals(300000L, NativeAudioLatencyEstimate.sample(48000, 48000, 1300000000, 1000000000, 48000))
    }
    @Test fun invalidAndStaleClocksCannotShiftPlayback() {
        assertNull(NativeAudioLatencyEstimate.sample(44100, 44100, 1, 4000000000, 44100))
        assertNull(NativeAudioLatencyEstimate.sample(44100, 0, 1000000000, 1000000000, 0))
        assertNull(NativeAudioLatencyEstimate.sample(44100, 88200, 1000000000, 1000000000, 44100))
        assertNull(NativeAudioLatencyEstimate.stable(listOf(10000, 300000, 600000)))
    }
    @Test fun stableJitterUsesTheMedianRatherThanADeviceConstant() {
        assertEquals(326000L, NativeAudioLatencyEstimate.stable(listOf(324000, 331000, 326000)))
        assertEquals(173000L, NativeAudioLatencyEstimate.stable(listOf(170000, 173000, 175000)))
        assertNull(NativeAudioLatencyEstimate.stable(listOf(326000, 327000)))
    }
    @Test fun automaticTimingAndUserAdjustmentRemainIndependent() {
        assertEquals(-326000L, NativeAudioLatencyEstimate.effective(0, 326000))
        assertEquals(-376000L, NativeAudioLatencyEstimate.effective(-50, 326000))
        assertEquals(0L, NativeAudioLatencyEstimate.effective(0, 0))
        assertEquals(-50000L, NativeAudioLatencyEstimate.effective(-50, 0))
    }
}
