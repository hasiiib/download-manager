"""Check version and installer configuration before expensive Tauri packaging."""
import importlib.util
import json
import pathlib
import re
import struct
import zlib
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

    def test_generated_preview_icon_has_valid_png_chunks(self):
        script = ROOT / "scripts/generate_icon.py"
        spec = importlib.util.spec_from_file_location("generate_icon", script)
        self.assertIsNotNone(spec)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        raw = module.icon_bytes()
        self.assertTrue(raw.startswith(bytes.fromhex("89504e470d0a1a0a")))
        offset = 8
        found = []
        while offset < len(raw):
            length = struct.unpack(">I", raw[offset:offset + 4])[0]
            kind = raw[offset + 4:offset + 8]
            payload = raw[offset + 8:offset + 8 + length]
            crc = struct.unpack(">I", raw[offset + 8 + length:offset + 12 + length])[0]
            self.assertEqual(crc, zlib.crc32(kind + payload) & 0xFFFFFFFF)
            if kind == b"IHDR":
                self.assertEqual(struct.unpack(">II", payload[:8]), (64, 64))
            found.append(kind)
            offset += 12 + length
        self.assertEqual(offset, len(raw))
        self.assertEqual(found, [b"IHDR", b"IDAT", b"IEND"])

    def test_local_tauri_command_prepares_icon(self):
        self.assertIn("generate_icon.py", self.frontend["scripts"].get("pretauri", ""))
        self.assertIn("tauri", self.frontend["scripts"])

    def test_ubuntu_installer_requires_aria2(self):
        self.assertIn("aria2", self.tauri["bundle"]["linux"]["deb"]["depends"])
        self.assertTrue(self.tauri["bundle"]["active"])


if __name__ == "__main__":
    unittest.main()
