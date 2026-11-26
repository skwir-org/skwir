"""Skwir's Homebrew cask (tools/package/cask.py, ADR-075, ADR-142) of a bundle."""

import hashlib
import json
import pathlib
import plistlib
import subprocess
import sys
import tempfile
import unittest
import zipfile

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import cask  # noqa: E402


def app_zip(path, minimum="12.0", plist=True):
    """A release's zip of the Mac app, its Info.plist naming `minimum`."""
    with zipfile.ZipFile(path, "w") as archive:
        archive.writestr("skwir.app/Contents/MacOS/skwir", b"")
        if plist:
            info = {"CFBundleIdentifier": "org.skwir.skwir", "CFBundleShortVersionString": "1.2.3"}
            if minimum is not None:
                info["LSMinimumSystemVersion"] = minimum
            archive.writestr("skwir.app/Contents/Info.plist", plistlib.dumps(info))
    return {"sha256": hashlib.sha256(path.read_bytes()).hexdigest().upper(), "bytes": path.stat().st_size}


def bundle_of(folder, assets=None, **app):
    """A release bundle of v1.2.3 in `folder`: the Mac app's zip (`app`: its
    Info.plist) and `assets` beside it, all in its manifest."""
    bundle = pathlib.Path(folder) / "v1.2.3"
    bundle.mkdir()
    assets = dict(assets or {})
    if app.pop("mac", True):
        assets["skwir-1.2.3-macos-arm64.zip"] = app_zip(bundle / "skwir-1.2.3-macos-arm64.zip", **app)
    (bundle / "manifest.json").write_text(json.dumps({
        "version": "1.2.3", "tag": "v1.2.3", "date": "2026-12-01", "assets": assets,
    }))
    return bundle


def tap_in(folder):
    tap = pathlib.Path(folder) / "homebrew-tap"
    (tap / ".git").mkdir(parents=True)
    return tap


def run(bundle, tap):
    return subprocess.run([sys.executable, HERE / "cask.py", bundle, "--tap", tap], capture_output=True, text=True)


def manifest_of(bundle):
    return json.loads((bundle / "manifest.json").read_text())


class Cask(unittest.TestCase):
    def test_the_release_s_zip_is_named_at_its_address_with_its_checksum(self):
        with tempfile.TemporaryDirectory() as directory:
            bundle = bundle_of(directory, {"skwir-1.2.3-macos-arm64.dmg": {"sha256": "cd" * 32, "bytes": 1}})
            sha256 = manifest_of(bundle)["assets"]["skwir-1.2.3-macos-arm64.zip"]["sha256"].lower()
            tap = tap_in(directory)
            made = run(bundle, tap)
            self.assertEqual(made.returncode, 0, made.stderr)
            text = (tap / "Casks/skwir.rb").read_text()
            self.assertIn('cask "skwir" do\n  version "1.2.3"\n  sha256 "' + sha256 + '"\n', text)
            # No `verified:`: Homebrew deprecated it (2026-11, cask/dsl.rb),
            # its own verification the default.
            self.assertIn('  url "https://github.com/skwir-org/skwir/releases/download/v#{version}/'
                          'skwir-#{version}-macos-arm64.zip"\n  name "Skwir"\n', text)
            self.assertNotIn("verified:", text)
            self.assertIn('  app "skwir.app"\n', text)
            # The program on the PATH, and `skw`, a link to it that starts
            # it as `skwir view` (ADR-151).
            self.assertIn('  binary "#{appdir}/skwir.app/Contents/MacOS/skwir"\n'
                          '  binary "#{appdir}/skwir.app/Contents/MacOS/skwir", target: "skw"\n', text)
            self.assertIn('  depends_on arch: :arm64\n  depends_on macos: :monterey\n', text)
            self.assertIn('  desc "Browse, find and organize your files"\n', text)
            self.assertIn("brew trust skwir-org/tap", text)
            # skwird, carried in the app, and Homebrew's taking its place.
            self.assertIn("skwir carries skwird", text)
            self.assertIn("      brew install skwir-org/tap/skwird\n", text)
            self.assertIn("Open Anyway", text)

    def test_the_cask_never_takes_the_quarantine_off(self):
        with tempfile.TemporaryDirectory() as directory:
            bundle = bundle_of(directory)
            text = cask.cask(bundle, manifest_of(bundle))
        for word in ("quarantine", "xattr", "preflight", "postflight", "--no-quarantine"):
            self.assertNotIn(word, text)

    def test_a_release_without_the_mac_app_or_its_tap_is_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            bundle = bundle_of(directory, {"skwir-1.2.3-windows-x86_64.msi": {"sha256": "ab" * 32}}, mac=False)
            tap = tap_in(directory)
            made = run(bundle, tap)
            self.assertNotEqual(made.returncode, 0)
            self.assertIn("no skwir-1.2.3-macos-arm64.zip", made.stderr)
            self.assertFalse((tap / "Casks/skwir.rb").exists())
            elsewhere = pathlib.Path(directory) / "elsewhere"
            elsewhere.mkdir()
            made = run(bundle, elsewhere)
            self.assertNotEqual(made.returncode, 0)
            self.assertIn("not a checkout", made.stderr)

    def test_the_oldest_macos_is_the_one_the_release_s_app_names(self):
        """Read from the zip the cask installs, whatever the working tree
        says now: a cask written again for an older release keeps its own."""
        with tempfile.TemporaryDirectory() as directory:
            bundle = bundle_of(directory, minimum="13.0")
            text = cask.cask(bundle, manifest_of(bundle))
        self.assertIn('  depends_on macos: :ventura\n', text)

    def test_an_app_naming_no_macos_known_or_a_zip_changed_is_refused(self):
        for app, said in [({"minimum": "11.0"}, "no Homebrew name known"),
                          ({"minimum": None}, "names no oldest macOS"),
                          ({"plist": False}, "no skwir.app/Contents/Info.plist read")]:
            with self.subTest(app=app), tempfile.TemporaryDirectory() as directory:
                bundle = bundle_of(directory, **app)
                tap = tap_in(directory)
                made = run(bundle, tap)
                self.assertNotEqual(made.returncode, 0)
                self.assertIn(said, made.stderr)
                self.assertFalse((tap / "Casks/skwir.rb").exists())
        with tempfile.TemporaryDirectory() as directory:
            bundle = bundle_of(directory)
            app_zip(bundle / "skwir-1.2.3-macos-arm64.zip", minimum="14.0")
            made = run(bundle, tap_in(directory))
            self.assertNotEqual(made.returncode, 0)
            self.assertIn("not the zip its manifest names", made.stderr)


if __name__ == "__main__":
    unittest.main()
