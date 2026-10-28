"""Check release-probe signing prerequisites without an emulator or app install."""

import importlib.util
import os
from pathlib import Path
import shutil
import sys
import tempfile
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location(
    "android_release_runtime_smoke", Path(__file__).with_name("android_release_runtime_smoke.py")
)
SMOKE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SMOKE)


class SigningReady(Exception):
    """Stop before alignment or installation once signing prerequisites are ready."""


class ReleaseRuntimeSigningTest(unittest.TestCase):
    def setUp(self):
        keytool = Path(os.environ["JAVA_HOME"]) / "bin/keytool" if os.environ.get("JAVA_HOME") \
            else Path(shutil.which("keytool") or "keytool")
        if not keytool.is_file():
            self.skipTest("A JDK with keytool is required")
        self.keytool = keytool.resolve()
        temporary = tempfile.TemporaryDirectory(prefix="lov-release-signing-test-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.home = self.root / "home"
        self.home.mkdir()
        self.keystore = self.home / ".android/debug.keystore"
        self.sdk = self.root / "sdk"
        android_jar = self.sdk / "platforms/android-36/android.jar"
        android_jar.parent.mkdir(parents=True)
        android_jar.touch()
        apksigner = self.sdk / "build-tools/36.0.0/apksigner"
        apksigner.parent.mkdir(parents=True)
        apksigner.touch()
        self.apk = self.root / "release-unsigned.apk"
        self.apk.touch()
        self.commands = []
        self.real_run = SMOKE.run

    def create_existing_key(self):
        self.keystore.parent.mkdir(parents=True)
        self.real_run(
            self.keytool, "-genkeypair", "-keystore", self.keystore,
            "-storepass", "android", "-keypass", "android",
            "-alias", "androiddebugkey", "-dname", "CN=Existing Test Key",
            "-keyalg", "RSA", "-keysize", "2048", "-validity", "10000",
            "-noprompt",
        )

    def run_until_signing(self, install_apk=True, prepare_only=False):
        def command(*arguments):
            self.commands.append(tuple(str(value) for value in arguments))
            if Path(arguments[0]).name == "keytool":
                return self.real_run(*arguments)
            self.assertEqual(Path(arguments[0]).name, "zipalign")
            self.assertTrue(self.keystore.is_file(), "Signing started without a debug key")
            raise SigningReady()

        arguments = ["release-runtime-smoke"]
        if prepare_only:
            arguments.append("--prepare-signing-key")
        else:
            arguments.extend(["--serial", "test-emulator"])
            if install_apk:
                arguments.extend(["--apk", str(self.apk)])
        with patch.object(Path, "home", return_value=self.home), \
                patch.dict(os.environ, {
                    "ANDROID_HOME": str(self.sdk),
                    "JAVA_HOME": str(self.keytool.parent.parent),
                }), \
                patch.object(sys, "argv", arguments), \
                patch.object(SMOKE, "run", side_effect=command):
            SMOKE.main()

    def test_creates_key_for_apk_in_fresh_home(self):
        with self.assertRaises(SigningReady):
            self.run_until_signing()
        self.assertEqual(sum(Path(command[0]).name == "keytool" for command in self.commands), 1)
        self.real_run(self.keytool, "-list", "-keystore", self.keystore,
                      "-storepass", "android", "-alias", "androiddebugkey")

    def test_prepares_key_before_build_and_reuses_it_for_apk(self):
        # CI prepares signing before installing an Android SDK or building APKs.
        unavailable_sdk = self.root / "sdk-not-installed"
        self.sdk.rename(unavailable_sdk)
        try:
            self.run_until_signing(prepare_only=True)
        finally:
            unavailable_sdk.rename(self.sdk)
        build_key = self.keystore.read_bytes()
        self.commands.clear()
        with self.assertRaises(SigningReady):
            self.run_until_signing()
        self.assertEqual(self.keystore.read_bytes(), build_key)
        self.assertFalse(any(Path(command[0]).name == "keytool" for command in self.commands))

    def test_reuses_existing_key_without_modifying_it(self):
        self.create_existing_key()
        before = self.keystore.read_bytes()
        with self.assertRaises(SigningReady):
            self.run_until_signing()
        self.assertEqual(self.keystore.read_bytes(), before)
        self.assertFalse(any(Path(command[0]).name == "keytool" for command in self.commands))

    def test_installed_app_without_key_requires_matching_key(self):
        with self.assertRaisesRegex(AssertionError, "Build a debug APK first"):
            self.run_until_signing(install_apk=False)
        self.assertFalse(self.keystore.exists())
        self.assertEqual(self.commands, [])


if __name__ == "__main__":
    unittest.main()
