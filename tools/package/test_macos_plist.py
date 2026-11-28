"""Both macOS package paths use the same viewer claim and bundle identity."""

from pathlib import Path
import plistlib
import subprocess
import sys
import tempfile
import unittest

GENERATOR = Path(__file__).with_name("macos-plist.py")


class MacBundle(unittest.TestCase):
    def test_release_and_prerelease_bundles(self):
        for version in ["0.7.3", "0.7.3-macos.1"]:
            with self.subTest(version=version), tempfile.TemporaryDirectory() as root:
                app = Path(root) / "A traveller's apps" / "skwir.app"
                done = subprocess.run(
                    [sys.executable, str(GENERATOR), str(app), version],
                    capture_output=True, text=True,
                )
                self.assertEqual(done.returncode, 0, done.stderr)
                info = plistlib.loads((app / "Contents/Info.plist").read_bytes())
                self.assertEqual(info["CFBundleIdentifier"], "org.skwir.skwir")
                self.assertEqual(info["CFBundleExecutable"], "skwir")
                self.assertEqual(info["CFBundleShortVersionString"], version)
                self.assertEqual(info["CFBundleVersion"], "0.7.3")
                self.assertEqual(info["LSMinimumSystemVersion"], "12.0")
                self.assertEqual(info["CFBundleDocumentTypes"], [{
                    "CFBundleTypeName": "Filesystem item",
                    "LSItemContentTypes": ["public.item"],
                    "CFBundleTypeRole": "Viewer",
                    "LSHandlerRank": "None",
                }])
                offered = GENERATOR.resolve().parents[2] / "tools/words/offered"
                languages = ["en", *sorted({
                    line.split()[0] for line in offered.read_text().splitlines()
                    if line.strip() and not line.startswith("#") and line.split()[0] != "en"
                })]
                self.assertEqual(info["CFBundleLocalizations"], languages)
                for language in languages:
                    strings = app / "Contents/Resources" / f"{language}.lproj/InfoPlist.strings"
                    self.assertIn('CFBundleDisplayName = "skwir";', strings.read_text())
                before = (app / "Contents/Info.plist").read_bytes()
                subprocess.run([sys.executable, str(GENERATOR), str(app), version], check=True)
                self.assertEqual((app / "Contents/Info.plist").read_bytes(), before)

    def test_invalid_version_does_not_write_a_bundle(self):
        with tempfile.TemporaryDirectory() as root:
            app = Path(root) / "skwir.app"
            done = subprocess.run(
                [sys.executable, str(GENERATOR), str(app), "not-a-version"],
                capture_output=True, text=True,
            )
            self.assertNotEqual(done.returncode, 0)
            self.assertFalse(app.exists())


if __name__ == "__main__":
    unittest.main()
