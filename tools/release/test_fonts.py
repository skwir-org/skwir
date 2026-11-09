"""Skwir's own fonts are the same in every package (ADR-118): the archives
tools/package/fonts.lock pins are those the Nix package fetches
(tools/package/nix/package.nix), and fonts.py takes files from each of
them and from nothing else."""

from pathlib import Path
from unittest import mock
import hashlib
import importlib.util
import shutil
import sys
import tempfile
import unittest
import zipfile

ROOT = Path(__file__).resolve().parents[2]
LOCK = ROOT / "tools/package/fonts.lock"


def pinned():
    for line in LOCK.read_text().splitlines():
        if line.strip() and not line.startswith("#"):
            yield line.split()


def fonts_py():
    spec = importlib.util.spec_from_file_location("fonts", ROOT / "tools/package/fonts.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class Fonts(unittest.TestCase):
    def test_the_nix_package_fetches_the_archives_pinned(self):
        package = (ROOT / "tools/package/nix/package.nix").read_text()
        for name, url, sum_ in pinned():
            with self.subTest(name):
                self.assertIn(f'"{name}"', package)
                self.assertIn(f'url = "{url}"', package)
                self.assertIn(f'sha256 = "{sum_}"', package)

    def test_fonts_py_takes_from_each_archive_pinned(self):
        taken = fonts_py().TAKEN
        self.assertEqual(sorted(taken), sorted(name for name, _, _ in pinned()))
        for name, members in taken.items():
            with self.subTest(name):
                files = [m for m in members if not isinstance(m, tuple)]
                licences = [m for m in members if isinstance(m, tuple)]
                self.assertTrue(files and all(f.endswith(".ttf") for f in files))
                self.assertEqual(len(licences), 1, "each family's licence")

    def test_only_what_the_lock_takes_is_carried(self):
        fonts = fonts_py()
        with tempfile.TemporaryDirectory() as temp:
            temp = Path(temp)
            archive = temp / "Font-1.zip"
            with zipfile.ZipFile(archive, "w") as zipped:
                zipped.writestr("ttf/Font-Regular.ttf", b"regular")
                zipped.writestr("OFL.txt", b"licence")
            sum_ = hashlib.sha256(archive.read_bytes()).hexdigest()
            lock = temp / "fonts.lock"
            lock.write_text(f"# a lock\nFont-1.zip https://example.invalid/Font-1.zip {sum_}\n")
            out = temp / "out"
            # Kept from before: the archive, and what an older lock took.
            (out / ".downloads").mkdir(parents=True)
            shutil.copy(archive, out / ".downloads/Font-1.zip")
            (out / "fonts").mkdir()
            (out / "fonts/Old-Regular.ttf").write_bytes(b"old")
            (out / "licenses/fonts/Old").mkdir(parents=True)
            (out / "licenses/fonts/Old/OFL.txt").write_text("old")
            fonts.LOCK = lock
            fonts.TAKEN = {"Font-1.zip": ["ttf/Font-Regular.ttf",
                                          ("OFL.txt", "licenses/fonts/Font/OFL.txt")]}
            with mock.patch.object(sys, "argv", ["fonts.py", str(out)]):
                fonts.main()
            carried = sorted(str(path.relative_to(out)) for path in out.rglob("*")
                             if path.is_file() and ".downloads" not in path.parts)
            self.assertEqual(carried, ["fonts/Font-Regular.ttf", "licenses/fonts/Font/OFL.txt"])
            self.assertTrue((out / ".downloads/Font-1.zip").is_file(), "the archive is kept")


if __name__ == "__main__":
    unittest.main()
