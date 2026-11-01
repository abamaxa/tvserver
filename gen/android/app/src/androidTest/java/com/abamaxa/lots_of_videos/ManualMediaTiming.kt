package com.abamaxa.lots_of_videos

/** Picture/PCM probes depend on emulator capture timing; run them explicitly. */
@Target(AnnotationTarget.FUNCTION)
@Retention(AnnotationRetention.RUNTIME)
annotation class ManualMediaTiming
