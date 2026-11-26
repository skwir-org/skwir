"""The desktop entries' and the AppStream metainfo's translations
(tools/package/linux/l10n.sh, ADR-139): po/skwir.pot is what the files
say, each language's PO is sound and merged with it, and they merge into
files that speak each language po/LINGUAS names (validated where
desktop-file-validate and appstreamcli are on PATH); a package's file
without one of them is told apart, and so is a package without Skwir
Preview's entry (ADR-151, 8), or with one speaking some languages only."""

from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import unittest

L10N = Path(__file__).resolve().parent / "linux/l10n.sh"
VIEWER = L10N.parent / "org.skwir.skwir.view.desktop"
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

    @unittest.skipUnless(GETTEXT, "gettext is in the Nix shell")
    def test_the_viewers_entry_is_merged_with_its_type(self):
        # What is not translated (its name, its program, its type) as the
        # English says it.
        with tempfile.TemporaryDirectory() as share:
            self.assertEqual(run("merge", share).returncode, 0)
            merged = Path(share, "applications", VIEWER.name).read_text().splitlines()
            english = VIEWER.read_text().splitlines()
            for key in ("MimeType=", "Exec=", "NoDisplay=", "Name="):
                with self.subTest(key=key):
                    self.assertEqual([row for row in merged if row.startswith(key)],
                                     [row for row in english if row.startswith(key)])

    @unittest.skipUnless(GETTEXT, "gettext is in the Nix shell")
    def test_the_viewers_entry_missing_or_short_of_a_language_is_told(self):
        with tempfile.TemporaryDirectory() as share:
            self.assertEqual(run("merge", share).returncode, 0)
            viewer = Path(share, "applications", VIEWER.name)
            # What it says in English alone: its translated rows left out.
            english = "".join(row for row in viewer.read_text().splitlines(keepends=True)
                              if not re.match(r"[A-Za-z-]*\[[^]]*\]=", row))
            viewer.unlink()
            told = run("installed", share)
            self.assertNotEqual(told.returncode, 0)
            self.assertIn("missing: the viewer's desktop entry", told.stdout)
            # Once it speaks one language, it speaks each.
            languages = [word for row in (L10N.parent / "po/LINGUAS").read_text().splitlines()
                         for word in row.split("#")[0].split()]
            viewer.write_text(english + f"GenericName[{languages[0]}]=…\n")
            told = run("installed", share)
            self.assertNotEqual(told.returncode, 0)
            self.assertIn(f"missing: the viewer's desktop entry in {languages[-1]}", told.stdout)
            viewer.write_text(english + "".join(f"GenericName[{language}]=…\n" for language in languages))
            installed = run("installed", share)
            self.assertEqual(installed.returncode, 0, installed.stdout)

    @unittest.skipUnless(GETTEXT, "gettext is in the Nix shell")
    def test_a_language_named_as_its_po_file_is_installed(self):
        # gettext 0.21 (Ubuntu 22.04's, the AppImage's) names a language in
        # the metainfo as its PO file does (zh_CN), not as BCP 47 does.
        with tempfile.TemporaryDirectory() as share:
            self.assertEqual(run("merge", share).returncode, 0)
            metainfo = Path(share, "metainfo/org.skwir.skwir.metainfo.xml")
            text = metainfo.read_text()
            older = re.sub(r'xml:lang="zh(-Hans)?-CN"', 'xml:lang="zh_CN"', text)
            self.assertNotEqual(older, text, "the metainfo says zh-CN or zh-Hans-CN")
            metainfo.write_text(older)
            installed = run("installed", share)
            self.assertEqual(installed.returncode, 0, installed.stdout)


if __name__ == "__main__":
    unittest.main()
