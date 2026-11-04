"""Skwir's own fonts are the same in every package (ADR-118): the archives
tools/package/fonts.lock pins are those the Nix package fetches, and
fonts.py takes files from each of them and from nothing else."""

from pathlib import Path
import importlib.util
import unittest

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
        flake = (ROOT / "flake.nix").read_text()
        for name, url, sum_ in pinned():
            with self.subTest(name):
                self.assertIn(f'"{name}"', flake)
                self.assertIn(f'url = "{url}"', flake)
                self.assertIn(f'sha256 = "{sum_}"', flake)

    def test_fonts_py_takes_from_each_archive_pinned(self):
        taken = fonts_py().TAKEN
        self.assertEqual(sorted(taken), sorted(name for name, _, _ in pinned()))
        for name, members in taken.items():
            with self.subTest(name):
                files = [m for m in members if not isinstance(m, tuple)]
                licences = [m for m in members if isinstance(m, tuple)]
                self.assertTrue(files and all(f.endswith(".ttf") for f in files))
                self.assertEqual(len(licences), 1, "each family's licence")


if __name__ == "__main__":
    unittest.main()
