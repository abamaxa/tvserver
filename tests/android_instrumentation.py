#!/usr/bin/env python3
"""Run one AndroidJUnitRunner shard using APKs built once by CI."""
import argparse
import os
from pathlib import Path
import re
import subprocess
import sys
import threading

TARGET = "com.abamaxa.lots_of_videos"
RUNNER = f"{TARGET}.test/androidx.test.runner.AndroidJUnitRunner"
TIMING_ANNOTATION = f"{TARGET}.ManualMediaTiming"


def instrumentation_succeeded(output):
    summary = re.search(r"^OK \((\d+) tests?\)\s*$", output, re.MULTILINE)
    codes = re.findall(r"^INSTRUMENTATION_CODE: (-?\d+)\s*$", output, re.MULTILINE)
    return bool(summary and int(summary[1]) > 0 and codes and codes[-1] == "-1"
                and "FAILURES!!!" not in output
                and not re.search(r"^INSTRUMENTATION_STATUS_CODE: -(?:1|2)\s*$", output, re.MULTILINE))


def run(adb, *arguments, timeout=120):
    result = subprocess.run([*adb, *map(str, arguments)], stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT, text=True, timeout=timeout)
    print(result.stdout, end="", flush=True)
    result.check_returncode()
    return result.stdout


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--serial", required=True)
    parser.add_argument("--apk-dir", type=Path, default=Path("android-test-apks"))
    parser.add_argument("--reports", type=Path, default=Path("android-device-reports"))
    parser.add_argument("--shard", type=int, choices=(1, 2, 3), required=True)
    parser.add_argument("--suite", choices=("regression", "media-timing"), default="regression")
    args = parser.parse_args()
    sdk = Path(os.environ.get("ANDROID_HOME") or os.environ["ANDROID_SDK_ROOT"])
    adb = [str(sdk / "platform-tools/adb"), "-s", args.serial]
    args.reports.mkdir(parents=True, exist_ok=True)

    # This is a fresh CI emulator. Remove only the disposable release probe's
    # target before installing the independently signed debug/test APK pair.
    if args.shard == 1 and args.suite == "regression":
        subprocess.run([sys.executable, str(Path(__file__).with_name("android_release_runtime_smoke.py")),
                        "--serial", args.serial, "--apk", str(args.apk_dir / "app-release.apk")], check=True)
        run(adb, "uninstall", TARGET)
    run(adb, "install", "-r", args.apk_dir / "app-debug.apk")
    run(adb, "install", "-r", args.apk_dir / "app-test.apk")
    run(adb, "logcat", "-c")
    command = [*adb, "shell", "am", "instrument", "-w", "-r"]
    if args.suite == "regression":
        command += ["-e", "numShards", "3", "-e", "shardIndex", str(args.shard - 1)]
    command += ["-e", "annotation" if args.suite == "media-timing" else "notAnnotation",
                TIMING_ANNOTATION, RUNNER]
    print(f"Running {args.suite}" + (f" shard {args.shard}/3" if args.suite == "regression" else ""), flush=True)
    with (args.reports / "logcat.txt").open("w") as logcat:
        capture = subprocess.Popen([*adb, "logcat", "-v", "threadtime"], stdout=logcat,
                                   stderr=subprocess.STDOUT)
        try:
            with (args.reports / "instrumentation.txt").open("w") as transcript:
                process = subprocess.Popen(command, stdout=subprocess.PIPE,
                                           stderr=subprocess.STDOUT, text=True)
                timeout = threading.Timer(600, process.kill)
                timeout.start()
                lines = []
                try:
                    for line in process.stdout:
                        print(line, end="", flush=True)
                        transcript.write(line)
                        transcript.flush()
                        lines.append(line)
                    process.wait()
                finally:
                    timeout.cancel()
                passed = process.returncode == 0 and instrumentation_succeeded("".join(lines))
            if not passed:
                # Keep evidence of an ANR/native teardown failure before the
                # emulator action shuts the device down.
                for name, arguments in (
                    ("last-anr.txt", ("shell", "dumpsys", "activity", "lastanr")),
                    ("anr-dropbox.txt", ("shell", "dumpsys", "dropbox", "--print", "data_app_anr")),
                ):
                    try:
                        (args.reports / name).write_text(run(adb, *arguments))
                    except (subprocess.SubprocessError, OSError) as error:
                        print(f"Could not capture {name}: {error}", file=sys.stderr)
                raise RuntimeError("Instrumentation did not complete successfully; see the shard reports")
        finally:
            capture.terminate()
            capture.wait(timeout=10)


if __name__ == "__main__":
    try:
        main()
    except (RuntimeError, OSError, subprocess.SubprocessError) as error:
        print(error, file=sys.stderr)
        sys.exit(1)
