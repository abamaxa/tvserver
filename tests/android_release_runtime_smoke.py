#!/usr/bin/env python3
"""Exercise first-use extraction inside the installed, minified Android app.

Requires JDK 21 and Android SDK 36. Existing installations need their matching
Android debug signing key. With --apk, a temporary copy is signed and installed as
an update; the standard debug key is created if it is missing on a fresh runner.
The probe uses fresh, disposable runtime storage without clearing the app's data.
"""
import argparse
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import zipfile

PROBE = "com.abamaxa.releaseprobe"
TARGET = "com.abamaxa.lots_of_videos"


def run(*arguments):
    return subprocess.check_output(
        [str(value) for value in arguments], stderr=subprocess.STDOUT,
        text=True, timeout=120,
    )


def prepare_debug_key(keystore, keytool):
    if keystore.is_file():
        return
    keystore.parent.mkdir(parents=True, exist_ok=True)
    run(keytool, "-genkeypair", "-keystore", keystore,
        "-storetype", "JKS", "-storepass", "android", "-keypass", "android",
        "-alias", "androiddebugkey", "-dname", "CN=Android Debug,O=Android,C=US",
        "-keyalg", "RSA", "-keysize", "2048", "-validity", "10000", "-noprompt")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--serial")
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--apk", type=Path)
    mode.add_argument("--prepare-signing-key", action="store_true",
                      help="Prepare the shared debug key before building APKs; only a JDK is needed")
    args = parser.parse_args()
    java_home = os.environ.get("JAVA_HOME")
    javac = Path(java_home) / "bin/javac" if java_home else Path(shutil.which("javac") or "javac")
    keystore = Path.home() / ".android/debug.keystore"
    if args.prepare_signing_key:
        prepare_debug_key(keystore, javac.with_name("keytool"))
        return
    if not args.serial:
        parser.error("--serial is required unless --prepare-signing-key is used")
    default_sdk = Path.home() / ("Library/Android/sdk" if sys.platform == "darwin" else "Android/Sdk")
    sdk = Path(os.environ.get("ANDROID_HOME") or os.environ.get("ANDROID_SDK_ROOT") or default_sdk)
    android_jar = sdk / "platforms/android-36/android.jar"
    build_tools = max(
        (path for path in (sdk / "build-tools").iterdir() if (path / "apksigner").is_file()),
        key=lambda path: tuple(map(int, re.findall(r"\d+", path.name))),
    )
    jar = javac.with_name("jar")
    adb = [sdk / "platform-tools/adb", "-s", args.serial]
    assert android_jar.is_file(), f"Android SDK 36 is missing: {android_jar}"
    if args.apk:
        prepare_debug_key(keystore, javac.with_name("keytool"))
    assert keystore.is_file(), f"Build a debug APK first to create {keystore}"

    with tempfile.TemporaryDirectory(prefix="lov-release-runtime-") as directory:
        work = Path(directory)

        def sign(source, name):
            aligned, signed = work / f"{name}-aligned.apk", work / f"{name}.apk"
            run(build_tools / "zipalign", "-f", "-P", "16", "4", source, aligned)
            run(build_tools / "apksigner", "sign", "--ks", keystore,
                "--ks-key-alias", "androiddebugkey", "--ks-pass", "pass:android",
                "--key-pass", "pass:android", "--v4-signing-enabled", "false",
                "--out", signed, aligned)
            return signed

        if args.apk:
            print(run(*adb, "install", "-r", sign(args.apk.resolve(), "target")), end="")

        classes, dex = work / "classes", work / "dex"
        classes.mkdir()
        dex.mkdir()
        source = Path(__file__).parent / "android/ReleaseRuntimeProbe.java"
        run(javac, "-source", "8", "-target", "8", "-bootclasspath", android_jar,
            "-d", classes, source)
        class_jar = work / "classes.jar"
        run(jar, "cf", class_jar, "-C", classes, ".")
        run(build_tools / "d8", "--lib", android_jar, "--min-api", "24",
            "--output", dex, class_jar)
        manifest = work / "AndroidManifest.xml"
        manifest.write_text(f'''<manifest xmlns:android="http://schemas.android.com/apk/res/android" package="{PROBE}">
  <uses-sdk android:minSdkVersion="24" android:targetSdkVersion="36" />
  <application android:label="Release runtime probe" android:hasCode="true" />
  <instrumentation android:name="{PROBE}.ReleaseRuntimeProbe" android:targetPackage="{TARGET}" />
</manifest>\n''')
        unsigned = work / "probe-unsigned.apk"
        run(build_tools / "aapt2", "link", "-I", android_jar,
            "--manifest", manifest, "-o", unsigned)
        with zipfile.ZipFile(unsigned, "a") as archive:
            archive.write(dex / "classes.dex", "classes.dex")
        installed = False
        try:
            run(*adb, "install", "-r", sign(unsigned, "probe"))
            installed = True
            result = run(*adb, "shell", "am", "instrument", "-w", "-r",
                         f"{PROBE}/{PROBE}.ReleaseRuntimeProbe")
            print(result, end="")
            if "INSTRUMENTATION_RESULT: result=PASS" not in result or "INSTRUMENTATION_CODE: -1" not in result:
                raise RuntimeError("Fresh release runtime initialization failed")
        finally:
            if installed:
                run(*adb, "uninstall", PROBE)


if __name__ == "__main__":
    try:
        main()
    except subprocess.CalledProcessError as error:
        print(error.output, file=sys.stderr)
        sys.exit(error.returncode)
    except (AssertionError, RuntimeError, OSError, subprocess.TimeoutExpired) as error:
        print(error, file=sys.stderr)
        sys.exit(1)
