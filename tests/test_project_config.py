"""Check version and installer configuration before expensive Tauri packaging."""
import json
import pathlib
import re
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]


class TestProjectConfiguration(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.frontend = json.loads((ROOT / "package.json").read_text(encoding="utf-8"))
        cls.tauri = json.loads((ROOT / "src-tauri/tauri.conf.json").read_text(encoding="utf-8"))
        cls.cargo = (ROOT / "src-tauri/Cargo.toml").read_text(encoding="utf-8")

    def test_versions_match(self):
        cargo_version = re.search(r"(?m)^version\s*=\s*\"([^\"]+)\"", self.cargo)
        self.assertIsNotNone(cargo_version)
        self.assertEqual(self.frontend["version"], cargo_version.group(1))
        self.assertEqual(self.tauri["version"], cargo_version.group(1))

    def test_bundle_uses_valid_icon_key_and_existing_generator(self):
        bundle = self.tauri["bundle"]
        self.assertNotIn("icons", bundle, "Tauri uses bundle.icon, not bundle.icons")
        self.assertEqual(bundle["icon"], ["icons/icon.png"])
        generator = (ROOT / "src-tauri/build.rs").read_text(encoding="utf-8")
        self.assertIn('dir.join("icon.png")', generator)

    def test_ubuntu_installer_requires_aria2(self):
        self.assertIn("aria2", self.tauri["bundle"]["linux"]["deb"]["depends"])
        self.assertTrue(self.tauri["bundle"]["active"])


if __name__ == "__main__":
    unittest.main()
