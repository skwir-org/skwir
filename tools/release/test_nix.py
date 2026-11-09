"""The Nix package (tools/package/nix/package.nix, ADR-124) builds what
every package ships: each program of tools/package/programs, those every
build carries in its list of helpers, and each other one under a flag of
its own, the helpers a package of their own holds elsewhere
(tools/package/optional) among them; and the notices it installs are the
source's. Written as nixpkgs writes a package, it names them itself rather
than reading the lists, so this keeps them alike."""

from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[2]
PACKAGE = ROOT / "tools/package/nix/package.nix"


def listed(name):
    text = (ROOT / "tools/package" / name).read_text()
    return {line.strip() for line in text.splitlines() if line.strip()}


def between(text, start, end):
    """The text from `start` to the first `end` after it."""
    at = text.index(start) + len(start)
    return text[at:text.index(end, at)]


def quoted(text):
    return set(re.findall(r'"([a-z0-9-]+)"', text))


class Package(unittest.TestCase):
    def setUp(self):
        self.text = PACKAGE.read_text()
        # `features = { withX = { … programs = [ … ]; … }; … };`, then
        # the helpers every build carries.
        self.features = between(self.text, "features = {", "enabled =")
        self.always = quoted(between(self.text, "helpers = [", "]"))
        self.flagged = set()
        for programs in re.findall(r"programs = \[([^\]]*)\]", self.features):
            self.flagged |= quoted(programs)

    def test_it_builds_every_program_shipped(self):
        self.assertIn('[ "skwir" ] ++ helpers', self.text, "skwir itself")
        self.assertEqual(self.always | self.flagged | {"skwir"}, listed("programs"))

    def test_the_flake_lays_out_what_a_start_runs_and_the_package_does_not(self):
        flake = (ROOT / "flake.nix").read_text()
        self.assertIn('linker=\\"tools/build/link-ordered\\"', flake,
                      "ADR-128, by --config: nixpkgs' build sets the linker's variable itself")
        self.assertIn("patchShebangs tools/build/link-ordered", flake, "a build's own shell")
        self.assertNotIn("link-ordered", self.text, "written to stand in nixpkgs, as ADR-124 says")

    def test_a_program_is_either_always_built_or_under_one_flag(self):
        self.assertFalse(self.always & self.flagged)
        found = [quoted(p) for p in re.findall(r"programs = \[([^\]]*)\]", self.features)]
        self.assertEqual(sum(len(p) for p in found), len(self.flagged), "one flag each")

    def test_what_a_package_of_its_own_holds_elsewhere_is_under_a_flag(self):
        self.assertLessEqual(listed("optional"), self.flagged)

    def test_every_flag_is_an_argument_with_its_default(self):
        flags = re.findall(r"^    (with[A-Z][A-Za-z]*) = \{", self.features, re.M)
        self.assertTrue(flags)
        for flag in flags:
            with self.subTest(flag):
                self.assertRegex(self.text, rf"\n  {flag} \? (true|false),")

    def test_the_notices_installed_are_the_sources(self):
        names = set()
        for notices in re.findall(r"notices = \[([^\]]*)\]", self.features):
            names |= quoted(notices)
        for name in names | {"skwir"}:
            with self.subTest(name):
                self.assertTrue((ROOT / "tools/package/notices" / name / "THIRD-PARTY-NOTICES").is_file())


if __name__ == "__main__":
    unittest.main()
