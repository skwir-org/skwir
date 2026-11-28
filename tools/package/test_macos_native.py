"""Downstream version stamping must not rewrite third-party dependencies."""

import importlib.util
from pathlib import Path
import tempfile
import unittest


SPEC = importlib.util.spec_from_file_location(
    "macos_native", Path(__file__).with_name("macos-native.py")
)
NATIVE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(NATIVE)


class VersionTest(unittest.TestCase):
    def test_workspace_version_and_lock_agree_without_changing_dependencies(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            root.joinpath("Cargo.toml").write_text(
                '[workspace.package]\nversion = "0.7.2"\n'
            )
            root.joinpath("Cargo.lock").write_text(
                'version = 4\n\n[[package]]\nname = "skwir"\nversion = "0.7.2"\n'
                '\n[[package]]\nname = "skwir-core"\nversion = "0.7.2"\n'
                '\n[[package]]\nname = "other"\nversion = "0.7.2"\n'
                'source = "registry+https://github.com/rust-lang/crates.io-index"\n'
            )
            previous, NATIVE.ROOT = NATIVE.ROOT, root
            try:
                NATIVE.set_version("0.7.3-macos.1")
                manifest = NATIVE.tomllib.loads(root.joinpath("Cargo.toml").read_text())
                packages = NATIVE.tomllib.loads(root.joinpath("Cargo.lock").read_text())["package"]
                self.assertEqual(manifest["workspace"]["package"]["version"], "0.7.3-macos.1")
                self.assertEqual([p["version"] for p in packages], [
                    "0.7.3-macos.1", "0.7.3-macos.1", "0.7.2"
                ])
                before = root.joinpath("Cargo.lock").read_bytes()
                NATIVE.set_version("0.7.3-macos.1")
                self.assertEqual(root.joinpath("Cargo.lock").read_bytes(), before)
            finally:
                NATIVE.ROOT = previous

    def test_invalid_version_is_rejected_before_any_files_change(self):
        with self.assertRaises(ValueError):
            NATIVE.set_version("0.7.3\nunrelated-setting = true")


if __name__ == "__main__":
    unittest.main()
