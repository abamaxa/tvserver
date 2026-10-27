package com.abamaxa.releaseprobe;

import android.app.Activity;
import android.app.Instrumentation;
import android.content.Context;
import android.content.ContextWrapper;
import android.content.SharedPreferences;
import android.os.Bundle;
import android.util.Log;
import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.IOException;
import java.io.InputStream;
import java.lang.reflect.InvocationTargetException;
import java.util.ArrayList;
import java.util.Iterator;
import java.util.List;
import java.util.UUID;
import org.json.JSONObject;

/** Platform-only instrumentation can call the real bridge in a minified APK. */
public class ReleaseRuntimeProbe extends Instrumentation {
    @Override
    public void onCreate(Bundle arguments) {
        super.onCreate(arguments);
        start();
    }

    @Override
    public void onStart() {
        Bundle result = new Bundle();
        File root = new File(getTargetContext().getCacheDir(),
                "release-runtime-probe-" + UUID.randomUUID());
        List<String> preferences = new ArrayList<>();
        Context isolated = new ContextWrapper(getTargetContext()) {
            @Override public Context getApplicationContext() { return this; }
            @Override public File getNoBackupFilesDir() { return directory("no-backup"); }
            @Override public File getCacheDir() { return directory("cache"); }
            @Override public File getFilesDir() { return directory("files"); }

            private File directory(String name) {
                File value = new File(root, name);
                if (!value.isDirectory() && !value.mkdirs()) {
                    throw new IllegalStateException("Could not create runtime fixture " + value);
                }
                return value;
            }

            @Override
            public SharedPreferences getSharedPreferences(String name, int mode) {
                String unique = root.getName() + "-" + name;
                if (!preferences.contains(unique)) preferences.add(unique);
                return super.getSharedPreferences(unique, mode);
            }
        };
        int status = Activity.RESULT_OK;
        try {
            // Match a JNI-attached Rust worker's initial class-loader state.
            Thread.currentThread().setContextClassLoader(null);
            Class<?> bridge = isolated.getClassLoader()
                    .loadClass("com.abamaxa.lots_of_videos.NativeBridge");
            String raw = (String) bridge.getMethod("prepareRuntime", Context.class)
                    .invoke(null, isolated);
            JSONObject runtime = new JSONObject(raw);
            JSONObject environment = runtime.getJSONObject("environment");
            String prefix = root.getCanonicalPath() + File.separator;
            if (!new File(runtime.getString("script")).getCanonicalPath().startsWith(prefix)
                    || !new File(environment.getString("PYTHONHOME"))
                            .getCanonicalPath().startsWith(prefix)) {
                throw new AssertionError("Runtime reused existing app storage instead of fresh storage");
            }
            ProcessBuilder builder = new ProcessBuilder(runtime.getString("python"), "-c",
                    "import sys,ssl;sys.path.insert(0,sys.argv[1]);import yt_dlp,yt_dlp_ejs;"
                            + "print('runtime-ok',yt_dlp.version.__version__)",
                    runtime.getString("script"));
            for (Iterator<String> keys = environment.keys(); keys.hasNext();) {
                String key = keys.next();
                builder.environment().put(key, environment.getString(key));
            }
            Process process = builder.redirectErrorStream(true).start();
            ByteArrayOutputStream bytes = new ByteArrayOutputStream();
            try (InputStream input = process.getInputStream()) {
                byte[] buffer = new byte[8192];
                int count;
                while ((count = input.read(buffer)) != -1) bytes.write(buffer, 0, count);
            }
            String output = bytes.toString("UTF-8");
            if (process.waitFor() != 0 || !output.contains("runtime-ok")) {
                throw new AssertionError(output);
            }
            result.putString("result", "PASS");
            result.putString("output", output.trim());
        } catch (Throwable error) {
            if (error instanceof InvocationTargetException && error.getCause() != null) {
                error = error.getCause();
            }
            result.putString("result", "FAIL");
            result.putString("error", Log.getStackTraceString(error));
            status = Activity.RESULT_CANCELED;
        } finally {
            for (String name : preferences) getTargetContext().deleteSharedPreferences(name);
            remove(root, root);
        }
        finish(status, result);
    }

    private void remove(File file, File root) {
        try {
            // Delete links themselves rather than recursing into external targets.
            String canonical = file.getCanonicalPath();
            String base = root.getCanonicalPath();
            if (canonical.equals(base) || canonical.startsWith(base + File.separator)) {
                File[] children = file.listFiles();
                if (children != null) {
                    for (File child : children) remove(child, root);
                }
            }
            file.delete();
        } catch (IOException error) {
            throw new IllegalStateException("Could not remove the runtime fixture", error);
        }
    }
}
