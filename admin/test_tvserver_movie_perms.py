import importlib.machinery
import importlib.util
import os
from pathlib import Path
import stat
import subprocess
import tempfile
import unittest


SCRIPT = Path(__file__).with_name("tvserver-movie-perms")
loader = importlib.machinery.SourceFileLoader("tvserver_movie_perms", str(SCRIPT))
spec = importlib.util.spec_from_loader(loader.name, loader)
permissions = importlib.util.module_from_spec(spec)
loader.exec_module(permissions)


class MoviePermissionsTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name) / "movies"
        self.root.mkdir()
        self.directory = self.root / "The House of Eliott"
        self.directory.mkdir()
        self.movie = self.directory / "episode.mp4"
        self.movie.write_bytes(b"video")
        permissions.ROOT = str(self.root)

    def tearDown(self):
        self.temp.cleanup()

    def test_chmod_changes_only_an_allowed_mode(self):
        permissions.apply("chmod", ["0664", str(self.movie)])
        self.assertEqual(stat.S_IMODE(self.movie.stat().st_mode), 0o664)
        with self.assertRaises(ValueError):
            permissions.apply("chmod", ["4755", str(self.movie)])
        with self.assertRaises(ValueError):
            permissions.apply("chmod", ["0777", str(self.movie)])

    def test_acl_applies_to_pinned_directory(self):
        permissions.apply("acl", ["chris", "rwx", str(self.directory)])
        result = subprocess.check_output(["/usr/bin/getfacl", "-cp", str(self.directory)], text=True)
        self.assertIn("user:chris:rwx", result)

    def test_default_acl_applies_to_directory_only(self):
        permissions.apply("default-acl", ["chris", "rwx", str(self.directory)])
        result = subprocess.check_output(["/usr/bin/getfacl", "-cp", str(self.directory)], text=True)
        self.assertIn("default:user:chris:rwx", result)
        with self.assertRaises(ValueError):
            permissions.apply("default-acl", ["chris", "rwx", str(self.movie)])

    def test_rejects_paths_outside_root_and_parent_traversal(self):
        outside = Path(self.temp.name) / "outside"
        outside.write_bytes(b"data")
        for path in (str(outside), str(self.root / ".." / "outside"), str(self.root) + "-other/file"):
            with self.subTest(path=path), self.assertRaises(ValueError):
                permissions.apply("chmod", ["0644", path])

    def test_rejects_symlinks_and_hardlinks(self):
        outside = Path(self.temp.name) / "outside"
        outside.write_bytes(b"data")
        (self.directory / "link").symlink_to(outside)
        (self.root / "linked-directory").symlink_to(self.directory, target_is_directory=True)
        for path in (self.directory / "link", self.root / "linked-directory" / "episode.mp4"):
            with self.subTest(path=path), self.assertRaises(OSError):
                permissions.apply("chmod", ["0644", str(path)])
        hardlink = self.directory / "hardlink.mp4"
        os.link(self.movie, hardlink)
        with self.assertRaises(ValueError):
            permissions.apply("chmod", ["0644", str(self.movie)])

    def test_rejects_other_acl_users(self):
        with self.assertRaises(ValueError):
            permissions.apply("acl", ["root", "rwx", str(self.directory)])


if __name__ == "__main__":
    unittest.main()
