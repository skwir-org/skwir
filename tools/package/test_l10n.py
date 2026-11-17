"""The desktop entry's and the AppStream metainfo's translations
(tools/package/linux/l10n.sh, ADR-139): po/skwir.pot is what the two files
say, each language's PO is sound and merged with it, and they merge into
files that speak each language po/LINGUAS names (validated where
desktop-file-validate and appstreamcli are on PATH); a package's file
without one of them is told apart."""

from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import unittest

L10N = Path(__file__).resolve().parent / "linux/l10n.sh"
GETTEXT = all(shutil.which(tool) for tool in ("msgfmt", "xgettext", "msgmerge", "msgcmp"))


def run(*arguments):
    return subprocess.run(["sh", str(L10N), *arguments], capture_output=True, text=True)


class Translations(unittest.TestCase):
    @unittest.skipUnless(GETTEXT, "gettext is in the Nix shell")
    def test_current_sound_and_merged(self):
        done = run("check")
        self.assertEqual(done.returncode, 0, done.stdout + done.stderr)

    @unittest.skipUnless(GETTEXT, "gettext is in the Nix shell")
    def test_a_language_missing_is_told(self):
        with tempfile.TemporaryDirectory() as share:
            merged = run("merge", share)
            self.assertEqual(merged.returncode, 0, merged.stderr)
            self.assertEqual(run("installed", share).returncode, 0)
            desktop = Path(share, "applications/org.skwir.skwir.desktop")
            desktop.write_text(re.sub(r"^\w+\[[^]]+\]=.*\n", "", desktop.read_text(), flags=re.M))
            told = run("installed", share)
            self.assertNotEqual(told.returncode, 0)
            self.assertIn("missing: the desktop entry or the metainfo in", told.stdout)


if __name__ == "__main__":
    unittest.main()
