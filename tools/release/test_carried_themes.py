"""The code themes skwir carries are the ones its notices name (ADR-071,
ADR-063): each theme file in crates/platform/assets/code-themes is compiled
into skwir-platform (highlight.rs's CARRIED), and tools/package/notices.py
names it (CARRIED_THEMES) under a licence kept beside it, which the
packages' notices carry. A theme added to one and not the other would ship
without its licence, and notices-check, which makes the notices from
notices.py's own list, would not see it."""

from pathlib import Path
import importlib.util
import re
import unittest

ROOT = Path(__file__).resolve().parents[2]
THEMES = ROOT / "crates/platform/assets/code-themes"


def notices_py():
    spec = importlib.util.spec_from_file_location("notices", ROOT / "tools/package/notices.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def theme_files():
    return sorted(path.name for path in THEMES.glob("*.tmTheme"))


class CarriedThemes(unittest.TestCase):
    def test_every_theme_there_is_compiled_in_and_nothing_else(self):
        source = (ROOT / "crates/platform/src/highlight.rs").read_text()
        included = re.findall(r'include_bytes!\("\.\./assets/code-themes/([^"]+)"\)', source)
        self.assertTrue(included)
        self.assertEqual(sorted(included), theme_files())

    def test_every_theme_is_named_in_the_notices_under_a_licence_beside_it(self):
        notices = notices_py()
        self.assertEqual(notices.CARRIED.resolve(), THEMES.resolve())
        under = {}
        for what, _, licence in notices.CARRIED_THEMES:
            self.assertTrue((THEMES / licence).is_file(), licence)
            for file in re.findall(r"[\w.-]+\.tmTheme", what):
                self.assertNotIn(file, under, "named once")
                under[file] = licence
        self.assertEqual(sorted(under), theme_files())
        self.assertEqual(set(under.values()),
                         {path.name for path in THEMES.glob("*.LICENSE")},
                         "each licence there is a theme's")


if __name__ == "__main__":
    unittest.main()
