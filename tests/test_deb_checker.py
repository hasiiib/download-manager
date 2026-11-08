"""Unit tests for offline Debian installer verification."""
import importlib.util
import pathlib
import shutil
import subprocess
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
    def check(self, paths=EXPECTED, dependencies="libc6 (>= 2.34), aria2, libgtk-3-0", version="0.2.0-1", package="download-manager"):
        with tempfile.TemporaryDirectory() as tmp:
            filename = pathlib.Path(tmp) / "app.deb"
            filename.write_bytes(b"test fixture")
            def fake_field(_package, field):
                return {
                    "Package": package,
                    "Version": version,
                    "Depends": dependencies,
                }[field]
            with patch.object(checker, "field", side_effect=fake_field):
                with patch.object(checker, "package_paths", return_value=paths):
                    checker.validate(filename)

    @unittest.skipUnless(shutil.which("dpkg-deb"), "dpkg-deb is required")
    def test_real_debian_archive_inspection(self):
        # Build a tiny local package so tar extraction and Debian metadata
        # parsing are tested without mocking any checker internals.
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            package_root = root / "package"
            control = package_root / "DEBIAN" / "control"
            control.parent.mkdir(parents=True)
            control.write_text(
                "Package: download-manager\\n"
                "Version: 0.2.0-1\\n"
                "Architecture: all\\n"
                "Maintainer: QA <qa@example.invalid>\\n"
                "Depends: aria2\\n"
                "Description: Offline Debian archive smoke test\\n",
                encoding="utf-8",
            )
            for name in EXPECTED:
                path = package_root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(b"local fixture")
            archive = root / "sample.deb"
            subprocess.run(
                ["dpkg-deb", "--build", str(package_root), str(archive)],
                check=True, stdout=subprocess.DEVNULL,
            )
            checker.validate(archive)

    def test_valid_package(self):
        self.check()

    def test_wrong_package_version_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "Unexpected package version"):
            self.check(version="9.9.9-1")

    def test_wrong_package_name_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "Unexpected package name"):
            self.check(package="different-app")

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
