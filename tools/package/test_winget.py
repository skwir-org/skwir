"""The winget manifests (tools/package/winget.py, ADR-142) of a bundle."""

import json
import pathlib
import subprocess
import sys
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import winget  # noqa: E402


class Winget(unittest.TestCase):
    def test_a_release_s_installer_is_named_at_its_address_with_its_checksum(self):
        with tempfile.TemporaryDirectory() as directory:
            bundle = pathlib.Path(directory) / "v1.2.3"
            bundle.mkdir()
            (bundle / "manifest.json").write_text(json.dumps({
                "version": "1.2.3", "tag": "v1.2.3", "date": "2026-12-01",
                "assets": {"skwir-1.2.3-windows-x86_64.msi": {"sha256": "ab" * 32, "bytes": 1}},
            }))
            subprocess.run([sys.executable, HERE / "winget.py", bundle], check=True, capture_output=True)
            folder = bundle.with_name("v1.2.3-winget") / "manifests/s/Skwir/Skwir/1.2.3"
            files = sorted(p.name for p in folder.iterdir())
            self.assertEqual(files, ["Skwir.Skwir.installer.yaml", "Skwir.Skwir.locale.en-US.yaml", "Skwir.Skwir.yaml"])
            installer = (folder / "Skwir.Skwir.installer.yaml").read_text()
            self.assertIn("  InstallerUrl: https://github.com/skwir-org/skwir/releases/download/v1.2.3/"
                          "skwir-1.2.3-windows-x86_64.msi\n", installer)
            self.assertIn("  InstallerSha256: " + "AB" * 32 + "\n", installer)
            self.assertIn("Scope: user\n", installer)
            self.assertIn("ReleaseDate: 2026-12-01\n", installer)
            self.assertIn(f"  UpgradeCode: \"{winget.upgrade_code()}\"\n", installer)
            locale = (folder / "Skwir.Skwir.locale.en-US.yaml").read_text()
            self.assertIn("ReleaseNotesUrl: https://skwir.org/releases/v1.2.3\n", locale)
            self.assertIn("LicenseUrl: https://github.com/skwir-org/skwir/blob/v1.2.3/LICENSE\n", locale)
            for text in (installer, locale, (folder / "Skwir.Skwir.yaml").read_text()):
                self.assertIn("PackageIdentifier: Skwir.Skwir\nPackageVersion: 1.2.3\n", text)
                self.assertTrue(text.startswith("# yaml-language-server: $schema=https://aka.ms/winget-manifest."))

    def test_a_scalar_that_yaml_would_misread_is_quoted(self):
        self.assertEqual(winget.quoted("Apache-2.0"), "Apache-2.0")
        self.assertEqual(winget.quoted("{6F1C2B7E}"), '"{6F1C2B7E}"')
        self.assertEqual(winget.quoted("a: b"), '"a: b"')
        self.assertEqual(winget.quoted("Browse, find"), '"Browse, find"')


if __name__ == "__main__":
    unittest.main()
