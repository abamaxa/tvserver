package com.abamaxa.lots_of_videos

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/** Tests installed APK assets, extracted libraries and executable paths together. */
@RunWith(AndroidJUnit4::class)
class BundledRuntimeTest {
    private fun runtime() = JSONObject(NativeBridge.prepareRuntime(
        InstrumentationRegistry.getInstrumentation().targetContext))

    private fun execute(runtime: JSONObject, program: String, vararg args: String): String {
        val environment = runtime.getJSONObject("environment")
        val command = mutableListOf("/system/bin/env")
        environment.keys().forEach { name -> command.add("$name=${environment.getString(name)}") }
        command.add(runtime.getString(program))
        command.addAll(args)
        val process = ProcessBuilder(command).redirectErrorStream(true).start()
        val output = process.inputStream.bufferedReader().use { it.readText() }
        assertEquals("$program failed: $output", 0, process.waitFor())
        return output
    }

    @Test
    fun pythonImportsBundledYtdlpSolverAndTlsWithoutPip() {
        val runtime = runtime()
        val code = "import sys,ssl;sys.path.insert(0,sys.argv[1]);import yt_dlp,yt_dlp_ejs;" +
            "print(sys.version);print(yt_dlp.version.__version__);print(yt_dlp_ejs.version);print(ssl.OPENSSL_VERSION)"
        val output = execute(runtime, "python", "-c", code, runtime.getString("script"))
        assertTrue(output.contains("3.12."))
        assertTrue(output.contains(runtime.getString("version")))
        assertTrue(output.contains("OpenSSL"))
    }

    @Test
    fun ffmpegFfprobeAndQuickjsAreExecutableFromTheApp() {
        val runtime = runtime()
        assertTrue(execute(runtime, "ffmpeg", "-version").contains("ffmpeg version"))
        assertTrue(execute(runtime, "ffprobe", "-version").contains("ffprobe version"))
        assertTrue(execute(runtime, "quickjs", "-e", "console.log('runtime-ok')").contains("runtime-ok"))
    }
}
