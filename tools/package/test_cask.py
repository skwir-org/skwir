"""Skwir's Homebrew cask (tools/package/cask.py, ADR-075, ADR-142) of a bundle."""

import json
import pathlib
import subprocess
import sys
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import cask  # noqa: E402


def bundle_of(folder, assets):
    bundle = pathlib.Path(folder) / "v1.2.3"
    bundle.mkdir()
    (bundle / "manifest.json").write_text(json.dumps({
        "version": "1.2.3", "tag": "v1.2.3", "date": "2026-12-01", "assets": assets,
    }))
    return bundle


class Cask(unittest.TestCase):
    def test_the_release_s_zip_is_named_at_its_address_with_its_checksum(self):
        with tempfile.TemporaryDirectory() as directory:
            bundle = bundle_of(directory, {"skwir-1.2.3-macos-arm64.zip": {"sha256": "AB" * 32, "bytes": 1},
                                           "skwir-1.2.3-macos-arm64.dmg": {"sha256": "cd" * 32, "bytes": 1}})
            tap = pathlib.Path(directory) / "homebrew-tap"
            (tap / ".git").mkdir(parents=True)
            subprocess.run([sys.executable, HERE / "cask.py", bundle, "--tap", tap], check=True, capture_output=True)
            text = (tap / "Casks/skwir.rb").read_text()
            self.assertIn('cask "skwir" do\n  version "1.2.3"\n  sha256 "' + "ab" * 32 + '"\n', text)
            self.assertIn('  url "https://github.com/skwir-org/skwir/releases/download/v#{version}/'
                          'skwir-#{version}-macos-arm64.zip",\n'
                          '      verified: "github.com/skwir-org/skwir/"\n', text)
            self.assertIn('  app "skwir.app"\n', text)
            self.assertIn('  depends_on arch: :arm64\n  depends_on macos: ">= :monterey"\n', text)
            self.assertIn('  desc "Browse, find and organize your files"\n', text)
            self.assertIn("brew trust skwir-org/tap", text)
            self.assertIn("Open Anyway", text)

    def test_the_cask_never_takes_the_quarantine_off(self):
        text = cask.cask({"version": "1.2.3", "tag": "v1.2.3",
                          "assets": {"skwir-1.2.3-macos-arm64.zip": {"sha256": "ab" * 32}}})
        for word in ("quarantine", "xattr", "preflight", "postflight", "--no-quarantine"):
            self.assertNotIn(word, text)

    def test_a_release_without_the_mac_app_or_its_tap_is_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            bundle = bundle_of(directory, {"skwir-1.2.3-windows-x86_64.msi": {"sha256": "ab" * 32}})
            tap = pathlib.Path(directory) / "homebrew-tap"
            (tap / ".git").mkdir(parents=True)
            made = subprocess.run([sys.executable, HERE / "cask.py", bundle, "--tap", tap],
                                  capture_output=True, text=True)
            self.assertNotEqual(made.returncode, 0)
            self.assertIn("no skwir-1.2.3-macos-arm64.zip", made.stderr)
            self.assertFalse((tap / "Casks/skwir.rb").exists())
            elsewhere = pathlib.Path(directory) / "elsewhere"
            elsewhere.mkdir()
            made = subprocess.run([sys.executable, HERE / "cask.py", bundle, "--tap", elsewhere],
                                  capture_output=True, text=True)
            self.assertNotEqual(made.returncode, 0)
            self.assertIn("not a checkout", made.stderr)

    def test_the_oldest_macos_is_the_app_s_own(self):
        self.assertEqual(cask.minimum_macos(), "monterey")


if __name__ == "__main__":
    unittest.main()
