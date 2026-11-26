"""Skwir Preview's types (tools/package/viewer_types.py, ADR-151, 8): its
desktop entry opens every type of a kind Skwir previews, as the database
Skwir carries groups them and as `classify` reads kinds, and a line that
says otherwise is told and made again."""

import pathlib
import subprocess
import sys
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import viewer_types  # noqa: E402

ENTRY = HERE / "linux/org.skwir.skwir.view.desktop"


def run(*arguments):
    return subprocess.run([sys.executable, "-I", HERE / "viewer_types.py", *arguments],
                          capture_output=True, text=True)


class ViewerTypes(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.database = viewer_types.Database()
        cls.by_icon = viewer_types.icon_kinds()
        cls.viewed = set(viewer_types.viewed(cls.database, cls.by_icon))

    def test_the_entry_opens_the_types_of_the_kinds_skwir_previews(self):
        checked = run("--check", ENTRY)
        self.assertEqual(checked.returncode, 0, checked.stderr)

    def test_a_line_changed_is_told_and_made_again(self):
        with tempfile.TemporaryDirectory() as folder:
            entry = pathlib.Path(folder, ENTRY.name)
            entry.write_text(ENTRY.read_text().replace("image/png;", ""))
            told = run("--check", entry)
            self.assertEqual(told.returncode, 1)
            self.assertIn("--write", told.stderr)
            self.assertEqual(run("--write", entry).returncode, 0)
            self.assertEqual(run("--check", entry).returncode, 0)
            self.assertEqual(entry.read_text(), ENTRY.read_text())

    def test_kinds_as_classify_reads_them(self):
        def kinds(name):
            return self.database.kinds(name, self.by_icon)

        self.assertEqual(kinds("image/png"), {"Image"})
        self.assertEqual(kinds("application/pdf"), {"Document"})
        self.assertEqual(kinds("application/zip"), {"Archive"})
        # Text by descent, code included; and by its top level alone.
        self.assertIn("Text", kinds("application/json"))
        self.assertIn("Text", kinds("text/x-python3"))
        # An alias is read as the type it names.
        self.assertEqual(kinds("application/x-shellscript"), kinds("text/x-shellscript"))
        self.assertEqual(kinds("inode/directory"), {"Folder"})
        self.assertEqual(kinds("application/x-executable"), {"Program"})

    def test_listed_by_every_name_what_a_desktop_entry_names(self):
        for name in ("image/png", "video/mp4", "audio/flac", "font/ttf", "text/markdown",
                     "application/vnd.oasis.opendocument.spreadsheet", "message/rfc822",
                     "text/x-shellscript"):
            self.assertIn(name, self.viewed)
        # By its aliases too: a system whose database is older than a
        # rename knows a type by its old name alone (Ubuntu 24.04's
        # shell scripts are application/x-shellscript).
        for name in ("application/x-shellscript", "video/x-matroska", "audio/x-flac",
                     "audio/x-wav", "image/x-icon"):
            self.assertIn(name, self.viewed)
        for name in ("inode/directory", "application/x-executable", "message/news",
                     "x-epoc/x-sisx-app", "zz-application/zz-winassoc-doc"):
            self.assertNotIn(name, self.viewed)
        names = {alias for name in self.database.types for alias in self.database.names[name]}
        self.assertEqual(self.viewed, names & self.viewed)
        # Each alias beside the type it names.
        self.assertEqual({self.database.unalias(name) for name in self.viewed} - self.viewed, set())

    def test_a_classify_read_otherwise_is_refused(self):
        source = viewer_types.CLASSIFY.read_text()
        for changed in (source.replace('m.as_str() == "text/plain"', 'm.is_text()'),
                        source.replace("Some(Kind::Image)", "Some(Kind::Picture)"),
                        source.replace("fn classify(", "fn kind_bits_of(")):
            with self.subTest(), tempfile.TemporaryDirectory() as folder:
                rust = pathlib.Path(folder, "mime.rs")
                rust.write_text(changed)
                with self.assertRaises(SystemExit):
                    viewer_types.icon_kinds(rust)


if __name__ == "__main__":
    unittest.main()
