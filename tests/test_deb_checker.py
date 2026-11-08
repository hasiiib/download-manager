"""Unit tests for offline Debian installer verification."""
import importlib.util
import pathlib
import tempfile
import unittest
from unittest.mock import patch

CHECKER = pathlib.Path(__file__).resolve().parents[1] / "scripts/check_deb.py"
spec = importlib.util.spec_from_file_location("check_deb", CHECKER)
checker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checker)

EXPECTED = [
    "usr/bin/download-manager",
    "usr/share/applications/dev.hasiiib.downloadmanager.desktop",
    "usr/share/icons/hicolor/64x64/apps/download-manager.png",
]


class TestDebChecker(unittest.TestCase):
    def check(self, paths=EXPECTED, dependencies="libc6 (>= 2.34), aria2, libgtk-3-0"):
        with tempfile.TemporaryDirectory() as tmp:
            filename = pathlib.Path(tmp) / "app.deb"
            filename.write_bytes(b"test fixture")
            def fake_field(_package, field):
                return {
                    "Package": "download-manager",
                    "Version": "0.2.0-1",
                    "Depends": dependencies,
                }[field]
            with patch.object(checker, "field", side_effect=fake_field):
                with patch.object(checker, "package_paths", return_value=paths):
                    checker.validate(filename)

    def test_valid_package(self):
        self.check()

    def test_missing_aria2_dependency(self):
        with self.assertRaisesRegex(ValueError, "aria2 dependency"):
            self.check(dependencies="libc6, libgtk-3-0")

    def test_missing_desktop_launcher(self):
        with self.assertRaisesRegex(ValueError, "desktop launcher"):
            self.check(paths=[path for path in EXPECTED if not path.endswith(".desktop")])

    def test_missing_executable(self):
        with self.assertRaisesRegex(ValueError, "main application executable"):
            self.check(paths=[path for path in EXPECTED if not path.startswith("usr/bin/")])

    def test_missing_icon(self):
        with self.assertRaisesRegex(ValueError, "application icon"):
            self.check(paths=[path for path in EXPECTED if not path.endswith(".png")])


if __name__ == "__main__":
    unittest.main()
