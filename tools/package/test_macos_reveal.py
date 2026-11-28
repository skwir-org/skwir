"""The live checker must reject partial results and leave pre-existing data alone."""

import importlib.util
from pathlib import Path
import sys
import tempfile
import unittest


SPEC = importlib.util.spec_from_file_location(
    "macos_reveal", Path(__file__).with_name("macos-reveal-check.py")
)
REVEAL = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = REVEAL
SPEC.loader.exec_module(REVEAL)


class RevealCheckerTest(unittest.TestCase):
    def test_only_complete_exact_selection_passes(self):
        folder = Path("/synthetic/fixture")
        names = ("one.txt", "two.txt")
        self.assertTrue(REVEAL.settled(REVEAL.Shown(str(folder), True, names, 2), folder, names))
        for shown in (
            None,
            REVEAL.Shown(str(folder), False, names, 2),
            REVEAL.Shown(str(folder / "one.txt"), True, names, 2),
            REVEAL.Shown(str(folder), True, ("one.txt",), 1),
            REVEAL.Shown(str(folder), True, names, 3),
        ):
            self.assertFalse(REVEAL.settled(shown, folder, names))

    def test_preexisting_residue_is_not_removed_after_setup_failure(self):
        with tempfile.TemporaryDirectory() as directory:
            fx = REVEAL.Fixture(Path(directory), "synthetic")
            existing = Path(directory) / "existing-data"
            existing.write_text("keep")
            fx.residues = lambda: [existing]
            with self.assertRaises(REVEAL.Setup):
                fx.build(Path(directory) / "unused.app", "skwir")
            REVEAL.forget_residues(fx)
            self.assertEqual(existing.read_text(), "keep")


if __name__ == "__main__":
    unittest.main()
