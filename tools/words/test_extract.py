"""tools/words/extract.py on small catalogs and a made-up tree: how much of
a language is translated and reviewed, which crates are on by default, and
when a language is offered and a release refuses it (ADR-139, 13)."""
import contextlib
import io
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))
import extract  # noqa: E402


class Messages(unittest.TestCase):
    def test_an_argument_inside_quotes_is_found_in_every_quoting(self):
        self.assertEqual(extract.quoted_arguments("“{ $path }” could not be made"), ["path"])
        self.assertEqual(
            extract.quoted_arguments("“{ $scheme }:” is no protocol, nor ‘{ $other }’ or \"{ $x }\" or «{ $y }»"),
            ["scheme", "other", "x", "y"],
        )
        self.assertEqual(extract.quoted_arguments("{ $file } has no “#{ $anchor }”, “{ $file }”"), ["anchor", "file"])
        # Quotes without an argument, an argument without quotes, an
        # apostrophe: nothing.
        self.assertEqual(extract.quoted_arguments("the “Trash” of { $name }’s"), [])
        self.assertEqual(extract.quoted_arguments("“{ $a }\n{ $b }”"), [], "quotes on one line")

    def test_a_draft_is_a_message_whose_own_comment_says_so(self):
        source = "\n".join(
            [
                "# French catalog of skwir-sample: every message is a draft until reviewed.",
                "",
                "# draft",
                "plain = Un",
                "",
                "reviewed = Deux",
                "",
                "# What it counts.",
                "# draft",
                "counted = { $n ->",
                "    [one] un élément",
                "   *[other] { $n } éléments",
                "}",
                "",
                "## A group's comment is no message's.",
                "grouped = Quatre",
                "",
                "# draft",
                "",
                "parted = Cinq, its comment a blank line above",
                "lines =",
                "    Six",
                "    sur deux lignes",
                "# draft",
                "after_lines = Sept",
                "# draft",
                "braced = { $n ->",
                "[one] un, its variant at the line's start",
                "*[other] { $n }",
                "}",
                "quoted = { \"{\" } then { $n }",
                "-term = No message",
            ]
        )
        self.assertEqual(
            extract.messages_of(source),
            {
                "plain": True,
                "reviewed": False,
                "counted": True,
                "grouped": False,
                "parted": False,
                "lines": False,
                "after_lines": True,
                "braced": True,
                "quoted": False,
            },
        )

    def test_an_id_written_twice_is_its_first(self):
        self.assertEqual(extract.messages_of("a = un\n# draft\na = autre\n"), {"a": False})
        self.assertEqual(extract.messages_of("# draft\na = un\na = autre\n"), {"a": True})

    def test_a_message_left_open_ends_where_fluent_begins_anew(self):
        source = "\n".join(
            [
                "a = { $n",
                "# draft",
                "b = ok",
                'c = { $n ->',
                '   *[other] say "x',
                "}",
                "d = fine",
                "e = { $n ->",
                "[one] un",
                "*[other] plus",
                "}",
                "    .gender = masculine",
                "-term = { $n",
                "f = last",
            ]
        )
        self.assertEqual(
            extract.messages_of(source),
            {"a": False, "b": True, "c": False, "d": False, "e": False, "f": False},
        )

    def test_french_writes_its_own_spaces_beside_its_punctuation(self):
        nbsp, narrow = " ", " "
        source = "\n".join(
            [
                "# A comment writes as it likes : here ; there",
                f"right = Aucun{nbsp}: rien{narrow}; vraiment{narrow}? oui{narrow}! «{narrow}nom{narrow}»",
                "unspaced = 14:02, type:image, `?`",
                "question = ? un caractère",
                "plain = Aucun : rien",
                f"swapped = Aucun{narrow}: rien{nbsp}; et ?",
                "lines =",
                "    ? au début d’une ligne",
                "    puis { $n } : après un argument",
                f"quoted = « nom{nbsp}»",
            ]
        )
        problems = extract.spacing_problems(Path("sample/fr.ftl"), "fr", source)
        self.assertEqual(
            [problem.split(": ", 1)[0] for problem in problems],
            [
                "sample/fr.ftl:5:14",
                "sample/fr.ftl:6:16",
                "sample/fr.ftl:6:23",
                "sample/fr.ftl:6:28",
                "sample/fr.ftl:9:16",
                "sample/fr.ftl:10:15",
                "sample/fr.ftl:10:11",
            ],
        )
        self.assertIn("`plain` wants a no-break space (U+00A0) before `:`", problems[0])
        self.assertIn("`lines` wants", problems[4])
        # Other languages write no such spaces here.
        self.assertEqual(extract.spacing_problems(Path("sample/de.ftl"), "de", "a = Keine : nichts"), [])

    def test_the_program_s_own_catalogs_are_read_whole(self):
        # Every message begins a line of its own: each is found, and every
        # draft, marked on the line above it.
        root = Path(__file__).resolve().parents[2]
        for path in sorted((root / "crates").glob("*/l10n/*.ftl")):
            text = path.read_text()
            found = extract.messages_of(text)
            lines = text.splitlines()
            begun = [n for n, line in enumerate(lines) if extract.MESSAGE.match(line)]
            self.assertEqual(len(found), len(begun), path)
            drafts = sum(1 for n in begun if n and lines[n - 1] == "# draft")
            self.assertEqual(sum(found.values()), drafts, path)


class Calls(unittest.TestCase):
    def test_a_noun_says_so_after_its_note(self):
        source = "\n".join(
            [
                'fn title() -> Words { words!(trash_title = "Trash", noun, note = "The place.") }',
                'fn last() -> Words { words!(shelf_title = "Shelf", noun) }',
                'fn other() -> Words { words!(moved = "Moved { $noun }", noun = n) }',
            ]
        )
        found = {id_: (english, note, noun) for id_, english, note, noun in extract.calls(source)}
        self.assertEqual(
            found,
            {
                "trash_title": ("Trash", "The place.", True),
                "shelf_title": ("Shelf", "", True),
                # An argument named noun is no flag.
                "moved": ("Moved { $noun }", "", False),
            },
        )
        written = extract.ftl({id_: found[id_] for id_ in found})
        self.assertIn("# The place.\n# noun\ntrash_title = Trash\n", written)
        self.assertIn("\n# noun\nshelf_title = Shelf\n", written)
        self.assertNotIn("# noun\nmoved", written)


class Crates(unittest.TestCase):
    def setUp(self):
        self.held = tempfile.TemporaryDirectory()
        self.addCleanup(self.held.cleanup)
        self.root = Path(self.held.name)

    def crate(self, name, source=""):
        folder = self.root / name
        (folder / "src").mkdir(parents=True)
        (folder / "Cargo.toml").write_text(f'[package]\nname = "{name}"\n')
        (folder / "src" / "lib.rs").write_text(source)
        return folder

    def test_a_plugin_is_on_unless_it_answers_false(self):
        program = self.crate("skwir-sample-core", "fn enabled_by_default(&self) -> bool { false }")
        self.assertTrue(extract.on_by_default(program, "skwir-sample-core"), "the program's own")
        silent = self.crate("skwir-plugin-silent", "// See `Plugin::enabled_by_default`.\n")
        self.assertTrue(extract.on_by_default(silent, "skwir-plugin-silent"))
        off = self.crate(
            "skwir-plugin-off",
            "impl Plugin for Off {\n    // fn enabled_by_default is said below.\n"
            "    fn enabled_by_default(&self) -> bool {\n        false\n    }\n}\n",
        )
        self.assertFalse(extract.on_by_default(off, "skwir-plugin-off"))
        on = self.crate("skwir-plugin-on", "fn enabled_by_default(&self) -> bool { true }")
        self.assertTrue(extract.on_by_default(on, "skwir-plugin-on"))
        unread = self.crate("skwir-plugin-unread", "fn enabled_by_default(&self) -> bool { cfg!(unix) }")
        with self.assertRaises(ValueError):
            extract.on_by_default(unread, "skwir-plugin-unread")

    def catalog(self, folder, language, text):
        (folder / "l10n").mkdir(exist_ok=True)
        (folder / "l10n" / f"{language}.ftl").write_text(text)

    def test_a_share_counts_every_crate_s_messages(self):
        core = self.crate("skwir-sample-core")
        off = self.crate("skwir-plugin-off", "fn enabled_by_default(&self) -> bool { false }")
        found = {
            core: {f"core_{n}": ("English", "") for n in range(4)},
            off: {f"off_{n}": ("English", "") for n in range(6)},
        }
        names = {core: "skwir-sample-core", off: "skwir-plugin-off"}
        # French: the program whole, the plugin off by default all but one.
        self.catalog(core, "fr", "".join(f"core_{n} = Un\n" for n in range(4)) + "gone = No English\n")
        self.catalog(off, "fr", "".join(f"off_{n} = Un\n" for n in range(4)) + "# draft\noff_4 = Un\n")
        # German: one of the program's still a draft; nothing of the plugin.
        self.catalog(core, "de", "# draft\ncore_0 = Eins\n" + "".join(f"core_{n} = Eins\n" for n in range(1, 4)))
        shares, problems = extract.shares(found, names)
        self.assertEqual(problems, [])
        self.assertEqual(
            shares,
            [
                {
                    "language": "de",
                    "messages": 10,
                    "translated": 4,
                    "reviewed": 3,
                    "default_whole": False,
                    "default_translated": True,
                    "name": "de",
                },
                {
                    "language": "fr",
                    "messages": 10,
                    "translated": 9,
                    "reviewed": 8,
                    "default_whole": True,
                    "default_translated": True,
                    "name": "fr",
                },
            ],
        )
        self.assertEqual([extract.percent(share, "translated") for share in shares], [40, 90])
        self.assertEqual([extract.percent(share, "reviewed") for share in shares], [30, 80])
        self.assertFalse(any(extract.meets_bar(share) for share in shares))
        # French is translated enough to be spoken as a draft; German not.
        self.assertEqual([extract.meets_draft_bar(share) for share in shares], [False, True])
        # One more reviewed: 90%, and all of what is on by default.
        self.catalog(off, "fr", "".join(f"off_{n} = Un\n" for n in range(5)))
        shares, _ = extract.shares(found, names)
        self.assertTrue(extract.meets_bar(shares[1]))
        # The plugin on by default, it would need all of it, translated
        # for a draft, reviewed for the bar.
        (off / "src" / "lib.rs").write_text("")
        shares, _ = extract.shares(found, names)
        self.assertFalse(extract.meets_bar(shares[1]))
        self.assertFalse(extract.meets_draft_bar(shares[1]), "off_5 untranslated")
        self.catalog(off, "fr", "".join(f"# draft\noff_{n} = Un\n" for n in range(6)))
        shares, _ = extract.shares(found, names)
        self.assertTrue(extract.meets_draft_bar(shares[1]), "all translated, drafts")
        self.assertFalse(extract.meets_bar(shares[1]))

    def test_a_language_is_named_as_its_catalog_of_core_says(self):
        core = self.crate("skwir-core")
        found = {core: {"settings_language_self": ("English", "")}}
        names = {core: "skwir-core"}
        self.catalog(core, "fr", "# draft\nsettings_language_self = Français\n")
        self.catalog(core, "zh-Hans", "settings_language_self = 简体中文\n")
        self.catalog(core, "de", "")
        shares, _ = extract.shares(found, names)
        # Said in itself, draft or not; its tag where core's catalog says none.
        self.assertEqual([share["name"] for share in shares], ["de", "Français", "简体中文"])
        self.assertEqual(extract.rust_string('a "b" \\c'), '"a \\"b\\" \\\\c"')

    def test_shares_round_down(self):
        share = {"messages": 1000, "translated": 999, "reviewed": 749}
        self.assertEqual(extract.percent(share, "translated"), 99)
        self.assertTrue(extract.under_floor(share))
        self.assertFalse(extract.under_floor({**share, "reviewed": 750}))
        self.assertEqual(extract.percent({"messages": 0, "translated": 0}, "translated"), 0)


class Offering(unittest.TestCase):
    """The whole tool on a made-up tree: a language offered as a draft once
    translated, reviewed once it meets the bar, kept between releases
    under it, refused by a release under 75% reviewed, or as a draft short
    of the draft's bar."""

    def setUp(self):
        self.held = tempfile.TemporaryDirectory()
        self.addCleanup(self.held.cleanup)
        self.root = Path(self.held.name)
        self.core = self.root / "crates" / "core"
        (self.core / "src").mkdir(parents=True)
        # A plugin off by default: what is on by default asks nothing of it.
        (self.core / "Cargo.toml").write_text('[package]\nname = "skwir-plugin-sample"\n')
        (self.root / "crates" / "skwir" / "src").mkdir(parents=True)
        (self.root / "tools" / "words").mkdir(parents=True)
        self.offered = self.root / "tools" / "words" / "offered"
        self.offered.write_text("# The languages offered.\n")
        self.quoted = self.root / "tools" / "words" / "quoted"
        for name, value in {
            "ROOT": self.root,
            "OFFERED": self.offered,
            "QUOTED": self.quoted,
            "CATALOGS_RS": self.root / "crates" / "skwir" / "src" / "catalogs.rs",
        }.items():
            patch = mock.patch.object(extract, name, value)
            patch.start()
            self.addCleanup(patch.stop)

    def say(self, count):
        """The crate says `count` messages."""
        calls = "".join(f'words!(said_{n} = "Said {n}");\n' for n in range(count))
        off = "fn enabled_by_default(&self) -> bool {\n    false\n}\n"
        (self.core / "src" / "lib.rs").write_text(f"{off}fn f() {{\n{calls}}}\n")

    def translate(self, count, reviewed):
        (self.core / "l10n").mkdir(exist_ok=True)
        text = "".join(("" if n < reviewed else "# draft\n") + f"said_{n} = Dit {n}\n" for n in range(count))
        (self.core / "l10n" / "fr.ftl").write_text(text)

    def run_tool(self, *flags):
        out, err = io.StringIO(), io.StringIO()
        with mock.patch.object(sys, "argv", ["extract.py", *flags]):
            with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
                code = extract.main()
        return code, out.getvalue() + err.getvalue()

    def test_a_language_offered_stays_until_a_release_refuses_it(self):
        self.say(10)
        self.translate(8, reviewed=8)
        code, said = self.run_tool()
        self.assertEqual(code, 0, said)
        self.assertEqual(extract.read_offered(self.offered), ({}, []))
        catalogs = extract.CATALOGS_RS.read_text()
        self.assertIn('language: "fr",\n        messages: 10,\n        translated: 8,\n        reviewed: 8,', catalogs)
        self.assertIn("offered: skwir_core::words::Offer::Incomplete", catalogs)
        # No catalog of core in this tree: named by its tag.
        self.assertIn('name: "fr",', catalogs)
        self.assertEqual(self.run_tool("--check")[0], 0)

        # Translated, 80% reviewed: offered as a draft, written in the record.
        self.translate(10, reviewed=8)
        code, said = self.run_tool("--check")
        self.assertEqual(code, 1, "the record and the catalogs' list are behind")
        self.assertIn("fr offered as a draft from now on (100% translated, 80% reviewed)", said)
        code, said = self.run_tool()
        self.assertEqual(code, 0, said)
        self.assertEqual(extract.read_offered(self.offered), ({"fr": True}, []))
        self.assertEqual(self.offered.read_text(), "# The languages offered.\nfr draft\n")
        self.assertIn("offered: skwir_core::words::Offer::Draft", extract.CATALOGS_RS.read_text())
        self.assertEqual(self.run_tool("--release")[0], 0)

        # 90% reviewed: offered, its draft mark gone from the record.
        self.translate(10, reviewed=9)
        code, said = self.run_tool()
        self.assertEqual(code, 0, said)
        self.assertIn("fr offered from now on (90% reviewed)", said)
        self.assertEqual(self.offered.read_text(), "# The languages offered.\nfr\n")
        self.assertIn("offered: skwir_core::words::Offer::Reviewed", extract.CATALOGS_RS.read_text())
        self.assertEqual(self.run_tool("--release")[0], 0)

        # Messages added untranslated: 75%, offered still, a release too.
        self.say(12)
        code, said = self.run_tool()
        self.assertEqual(code, 0, said)
        self.assertIn("offered: skwir_core::words::Offer::Reviewed", extract.CATALOGS_RS.read_text())
        self.assertEqual(self.run_tool("--release")[0], 0)

        # Under 75%: offered between releases, with a warning; a release
        # refuses it.
        self.say(13)
        code, said = self.run_tool()
        self.assertEqual(code, 0, said)
        self.assertIn("warning: fr is offered with only 69%", said)
        self.assertIn("offered: skwir_core::words::Offer::Reviewed", extract.CATALOGS_RS.read_text())
        self.assertEqual(self.run_tool("--check")[0], 0)
        code, said = self.run_tool("--release")
        self.assertEqual(code, 1)
        self.assertIn("a release offers none under 75%", said)

        # Taken out of the record: incomplete again, until it meets a bar.
        self.offered.write_text("# The languages offered.\n")
        code, said = self.run_tool()
        self.assertEqual(code, 0, said)
        self.assertIn("offered: skwir_core::words::Offer::Incomplete", extract.CATALOGS_RS.read_text())
        self.assertEqual(extract.read_offered(self.offered), ({}, []))
        self.assertEqual(self.run_tool("--release")[0], 0)

    def test_a_draft_short_of_its_bar_stays_until_a_release_refuses_it(self):
        self.say(10)
        self.translate(10, reviewed=0)
        self.assertEqual(self.run_tool()[0], 0)
        self.assertEqual(extract.read_offered(self.offered), ({"fr": True}, []))
        # A message added, not drafted: 90% translated, but not all of the
        # crate (what is on by default would need it): offered as a draft
        # still, said in English, and a release refuses it.
        (self.core / "src" / "lib.rs").write_text('fn f() {\n' + "".join(
            f'words!(said_{n} = "Said {n}");\n' for n in range(11)) + '}\n')
        code, said = self.run_tool()
        self.assertEqual(code, 0, said)
        self.assertIn("warning: fr is offered as a draft with 90% of its messages translated and not all", said)
        self.assertIn("offered: skwir_core::words::Offer::Draft", extract.CATALOGS_RS.read_text())
        code, said = self.run_tool("--release")
        self.assertEqual(code, 1)
        self.assertIn("draft what it lacks", said)
        # Drafted: a release takes it again.
        self.translate(11, reviewed=0)
        self.assertEqual(self.run_tool()[0], 0)
        self.assertEqual(self.run_tool("--release")[0], 0)

    def test_a_plugin_from_outside_keeps_its_catalogs_its_own(self):
        # Its catalogs, written by its author beside its manifest, are said
        # by the host while it is on: never compiled in, never counted,
        # never made from code, its English never "a crate saying nothing".
        self.say(10)
        self.translate(10, reviewed=10)
        plugin = self.root / "crates" / "plugins-outside" / "sample"
        (plugin / "src").mkdir(parents=True)
        (plugin / "Cargo.toml").write_text('[package]\nname = "skwir-outside-sample"\n')
        (plugin / "src" / "lib.rs").write_text('// Its messages: Words::message("hello").\n')
        (plugin / "l10n").mkdir()
        (plugin / "l10n" / "en.ftl").write_text("# Its author's English.\nhello = Hello\nbye = Bye\n")
        (plugin / "l10n" / "fr.ftl").write_text("# draft\nhello = Bonjour\n")
        (plugin / "l10n" / "de.ftl").write_text("hello = Hallo\n")
        code, said = self.run_tool()
        self.assertEqual(code, 0, said)
        catalogs = extract.CATALOGS_RS.read_text()
        self.assertNotIn("plugins-outside", catalogs)
        self.assertNotIn("skwir-outside-sample", catalogs)
        self.assertNotIn('language: "de"', catalogs, "no language of its own counted")
        self.assertIn('language: "fr",\n        messages: 10,\n        translated: 10,\n        reviewed: 10,', catalogs)
        self.assertEqual(
            (plugin / "l10n" / "en.ftl").read_text(),
            "# Its author's English.\nhello = Hello\nbye = Bye\n",
            "its English is its author's",
        )
        self.assertEqual(self.run_tool("--check"), (0, ""))
        self.assertTrue(extract.outside(plugin / "l10n" / "en.ftl"))
        self.assertFalse(extract.outside(self.core / "l10n" / "fr.ftl"))

    def test_a_record_line_is_a_tag_then_draft_or_nothing(self):
        self.say(2)
        self.translate(2, reviewed=0)
        self.offered.write_text("# The languages offered.\nfr maybe\n")
        code, said = self.run_tool()
        self.assertEqual(code, 1)
        self.assertIn("`fr maybe`: a line is a tag, then `draft` or nothing", said)

    def test_an_english_quoting_its_argument_is_refused_unless_listed(self):
        def write(*calls):
            (self.core / "src" / "lib.rs").write_text("fn f() {\n" + "".join(f"{call};\n" for call in calls) + "}\n")

        write('words!(gone = "“{ $path }” is gone", path = p)')
        code, said = self.run_tool()
        self.assertEqual(code, 1)
        self.assertIn("crates/core: `gone` writes quotes around `{ $path }` itself", said)
        # Listed, as those the English quoted when the check came.
        self.quoted.write_text("# Left to convert.\ncrates/core gone path\n")
        self.assertEqual(self.run_tool()[0], 0)
        self.assertEqual(self.run_tool("--check"), (0, ""))
        # Converted: its line quotes nothing, and the list only shrinks.
        write('words!(gone = "{ $path } is gone", path = Name(p))')
        code, said = self.run_tool("--check")
        self.assertEqual(code, 1)
        self.assertIn("tools/words/quoted: `crates/core gone path` quotes nothing any more", said)
        self.quoted.write_text("# Left to convert.\n")
        self.assertEqual(self.run_tool()[0], 0)
        self.assertEqual(self.run_tool("--check"), (0, ""))
        # A line that is not three words is said so.
        self.quoted.write_text("crates/core gone\n")
        code, said = self.run_tool("--check")
        self.assertEqual(code, 1)
        self.assertIn("a line is a crate's folder, an id and an argument", said)

    def test_the_record_names_only_languages_a_catalog_is_in(self):
        self.say(2)
        self.translate(2, reviewed=0)
        self.offered.write_text("# The languages offered.\nde\n")
        code, said = self.run_tool()
        self.assertEqual(code, 1)
        self.assertIn("de is no language a catalog is in", said)


if __name__ == "__main__":
    unittest.main()
