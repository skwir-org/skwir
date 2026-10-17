"""The AUR's PKGBUILDs agree (tools/package/aur): skwir-git builds and
installs what the release's PKGBUILD does, skwir-bin stands for the
packages that one makes, and each .SRCINFO is makepkg's own."""

from pathlib import Path
import re
import shutil
import subprocess
import unittest

AUR = Path(__file__).resolve().parents[1] / "package/aur"
RELEASE = AUR / "PKGBUILD"
GIT = AUR / "skwir-git/PKGBUILD"
BIN = AUR / "skwir-bin/PKGBUILD"


def shell(pkgbuild, script):
    """What bash says once it has read a PKGBUILD."""
    return subprocess.run(["bash", "-c", f'. "$1" && {script}', "_", str(pkgbuild)],
                          check=True, capture_output=True, text=True).stdout


def array(pkgbuild, name):
    return shell(pkgbuild, f'printf "%s\\n" "${{{name}[@]}}"').splitlines()


def function(pkgbuild, name):
    """A function's body as bash prints it, what skwir-git's changes on
    purpose made alike: the folder it builds in, what it provides and
    conflicts with, its packages' names."""
    body = []
    for line in shell(pkgbuild, f"declare -f {name}").splitlines()[1:]:
        if re.match(r"\s*(provides|conflicts)=", line):
            continue
        line = line.replace('cd "$pkgbase-$pkgver"', "cd skwir")
        body.append(re.sub(r"\b(skwir(?:-[a-z]+)?)-git\b", r"\1", line))
    return body


def srcinfo(pkgbuild, package, key):
    """A package's values in a .SRCINFO, its base's where it has none."""
    sections, current = {}, None
    for line in (pkgbuild.parent / ".SRCINFO").read_text().splitlines():
        if match := re.fullmatch(r"pkg(?:base|name) = (.+)", line):
            current = sections.setdefault(match[1], {})
        elif match := re.fullmatch(r"\t(\w+) = (.*)", line):
            current.setdefault(match[1], []).append(match[2])
    base = next(iter(sections.values()))
    return sections[package].get(key, base.get(key, []))


class Pkgbuilds(unittest.TestCase):
    def test_git_builds_and_installs_as_the_release(self):
        names = array(RELEASE, "pkgname")
        self.assertEqual(array(GIT, "pkgname"), [f"{name}-git" for name in names])
        for name in ("build", *(f"package_{n}" for n in names)):
            twin = name if name == "build" else f"{name}-git"
            with self.subTest(function=name):
                self.assertEqual(function(GIT, twin), function(RELEASE, name))
            if name != "build":
                text = shell(GIT, f"declare -f {twin}")
                package = name.removeprefix("package_")
                self.assertIn(f'provides=("{package}=$pkgver");', text)
                self.assertIn(f"conflicts=('{package}');", text)
        self.assertEqual(array(GIT, "makedepends"), ["git", *array(RELEASE, "makedepends")])
        for name in ("arch", "options", "_pdfium", "source_x86_64", "source_aarch64",
                     "sha256sums_x86_64", "sha256sums_aarch64"):
            with self.subTest(variable=name):
                self.assertEqual(array(GIT, name), array(RELEASE, name))

    def test_bin_stands_for_the_release_packages(self):
        names = array(RELEASE, "pkgname")
        self.assertEqual(array(BIN, "conflicts"), names)
        self.assertEqual(array(BIN, "provides"), [f"{name}={array(RELEASE, 'pkgver')[0]}" for name in names])
        self.assertEqual(array(BIN, "pkgver"), array(RELEASE, "pkgver"))
        # What it leaves to the system reads as the release's skwir says it.
        self.assertLessEqual(set(srcinfo(BIN, "skwir-bin", "optdepends")),
                             set(srcinfo(RELEASE, "skwir", "optdepends")))

    @unittest.skipUnless(shutil.which("makepkg"), "makepkg is available in the Linux Nix shell")
    def test_srcinfo_is_makepkg_own(self):
        for pkgbuild in (RELEASE, GIT, BIN):
            with self.subTest(pkgbuild=str(pkgbuild.relative_to(AUR))):
                made = subprocess.run(["makepkg", "--printsrcinfo"], cwd=pkgbuild.parent,
                                      check=True, capture_output=True, text=True).stdout
                self.assertEqual((pkgbuild.parent / ".SRCINFO").read_text(), made,
                                 "run `just package-aur`")


if __name__ == "__main__":
    unittest.main()
