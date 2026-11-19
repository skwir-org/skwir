#!/usr/bin/env python3
r"""Each crate's English catalog, made from its code (ADR-139, 2).

Every `words!(id = "English", …, note = "…")` of a crate's sources is a
message of that crate: written to `<crate>/l10n/en.ftl`, sorted by id, its
note as a comment above it. Never edited by hand. A noun of Skwir's
(`words!(trash_title = "Trash", noun)`) says so on a comment line of its own,
`noun`, after its note: a translation may give it its gender and forms
(ADR-139, 5).

    tools/words/extract.py             write every crate's en.ftl
    tools/words/extract.py --check     fail where one is not what the code says
    tools/words/extract.py --release   the same, and fail where a language
                                       offered is under 75% reviewed, or one
                                       offered as a draft is short of the
                                       draft's bar

It also writes the program's list of shipped catalogs,
crates/skwir/src/catalogs.rs: every crate's `l10n/<language>.ftl` but
English, compiled in (ADR-139, G); and how much of each language they say,
counted here so that the program reads nothing to know it (ADR-139, 13).

When a language is offered (ADR-139, 13): Automatic speaks a language once
it is in the record of languages offered, `tools/words/offered`. This tool
writes one there as a draft (`zh-Hans draft`) once 90% of all Skwir's
messages are translated in it, and every message of what is on by default
is: the program's own crates and each plugin on by default; the window then
says it is a draft. It writes it there as offered (`fr`), its draft mark
gone, once as much is reviewed (a message is reviewed once the `# draft`
comment above it is gone). Only a person takes one out, which sends it back
to incomplete: a release refuses a language offered under 75% reviewed, and
one offered as a draft short of the draft's bar (`--release`: its new
messages are drafted before a release), while between releases each stays
offered, its missing messages said in English.

A plugin from outside Skwir that this workspace holds
(`crates/plugins-outside/*`, ADR-045, ADR-046) brings its own catalogs
beside its manifest, its English written by its author, said by the host
while it is on (protocol 2, ADR-139, 14): this tool leaves them alone. They
are never compiled into the program, nor counted in a language's share,
nor made from code; the catalogs' test still holds each to its `en.ftl`.

Refused, in either mode: an id with two Englishes in one crate, and an
English that parts a number from its unit, or a dimension's numbers from
their `×`, by a plain space, where a line may break: a no-break space
joins them (`{ $n }\u{a0}MB`, `{ $width }\u{a0}×\u{a0}{ $height }`); and an
English that writes a space before a percent sign, which standard
English writes right after its number (`{ $n }%`); and an English that
writes quotes around an argument itself (`“{ $path }”`): a name's quotes
are its language's (ADR-139, 5), so the argument is a `Name`, a `Folder`
or a `Path`, or is said unquoted. Those the English quoted when this check
came are listed in `tools/words/quoted`, for a sweep to take out: the list
never grows, and a line there that quotes nothing any more is refused too.
Messages of tests (in a tests.rs or a tests/ folder, or whose id begins
with `test_`) are not a crate's. Also refused: a plugin whose
`enabled_by_default` answers anything but a literal, and a record naming a
language no catalog is in.
"""

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
HEADER = "# Made from the code by `just words` (tools/words/extract.py): never edit by hand.\n"
CALL = re.compile(r"\bwords!\s*\(")

# The record of the languages offered, and the shares that move a language
# in (a percent of all messages reviewed) and that a release refuses under.
OFFERED = ROOT / "tools" / "words" / "offered"
OFFER_AT = 90
KEEP_AT = 75
# A plugin's crate (by its package name): on by default unless its plugin
# answers `enabled_by_default` with `false`, a literal this tool reads.
PLUGIN_CRATE = "skwir-plugin-"
DEFAULT_SAID = re.compile(r"\bfn\s+enabled_by_default\b")
DEFAULT_ANSWER = re.compile(r"fn\s+enabled_by_default\s*\(\s*&\s*self\s*\)\s*->\s*bool\s*\{\s*(true|false)\s*\}")
# A message's first line in a catalog: its id at the line's start, then `=`.
MESSAGE = re.compile(r"([a-zA-Z][a-zA-Z0-9_-]*) *=")
# The flag that marks a noun, alone among a call's arguments, and the comment
# line that says so in the English catalog.
NOUN = re.compile(r",\s*noun\s*(?=[,)])")
NOUN_LINE = "noun"

# A number (an argument, or digits) and a unit's symbol after it, or a
# dimension's `×`, parted by a plain space. Units written as words (pixels,
# lines) are prose, which may break; `in` (inches) is too much an English
# word to tell.
NUMBER = r"(?:\{ *\$[a-z0-9_]+ *\}|\d)"
UNIT = (
    r"(?:[kKMGTP]i?B(?:/s)?|B|s|ms|µs|min|h|px|pt|ems?|µm|mm|cm|dm|m|km|mi|ft|yd|mil|NM"
    r"|[mck]?m[²³]|in[²³]|ft[²³]|dpi|ppi|MP|fps|k?Hz|[kM]bps|km/h|mph|k?g|°C?)(?![\w²³/'’])"
    r"|\{ *\$[a-z0-9_]*unit *\}"
)
# Digits before an argument: the argument is their unit (`1 { $next }`).
PARTED = re.compile(rf"{NUMBER} (?:{UNIT})|\d \{{ *\$|{NUMBER} ×|× {NUMBER}")
# A space of any kind before a percent sign: English writes `42%`.
SPACED_PERCENT = re.compile(rf"{NUMBER}[ \u00a0\u202f]%")

# The arguments an English quoted itself when the check came, each line a
# crate's folder, a message's id and the argument: never more.
QUOTED = ROOT / "tools" / "words" / "quoted"
# Quotes the English writes, on one line, and an argument inside them.
QUOTES = re.compile(r'“[^”\n]*”|‘[^’\n]*’|"[^"\n]*"|«[^»\n]*»')
ARGUMENT = re.compile(r"\{ *\$([a-z0-9_]+) *\}")


def quoted_arguments(english):
    """The arguments `english` writes quotes around itself (`“{ $path }”`,
    `“{ $scheme }:”`), each once, in the order written."""
    found = []
    for quoted in QUOTES.finditer(english):
        for argument in ARGUMENT.findall(quoted.group(0)):
            if argument not in found:
                found.append(argument)
    return found


def read_quoted(path):
    """The arguments `path` lets an English quote itself, as (crate folder,
    id, argument); and the problems met."""
    if not path.exists():
        return set(), []
    listed, problems = set(), []
    for line in path.read_text().splitlines():
        words = line.split()
        if not words or words[0].startswith("#"):
            continue
        if len(words) != 3:
            problems.append(f"{shown(path)}: `{line.strip()}`: a line is a crate's folder, an id and an argument")
            continue
        listed.add(tuple(words))
    return listed, problems


def quoted_problems(found, listed):
    """Each argument an English of `found` quotes itself that `listed`
    lets not, and each line of `listed` that quotes nothing any more."""
    problems, seen = [], set()
    for folder, messages in sorted(found.items()):
        for id_, (english, _, _) in sorted(messages.items()):
            for argument in quoted_arguments(english):
                key = (str(shown(folder)), id_, argument)
                seen.add(key)
                if key not in listed:
                    problems.append(
                        f"{key[0]}: `{id_}` writes quotes around `{{ ${argument} }}` itself: a name's"
                        " quotes are its language's (ADR-139, 5): give it as a `Name` (a `Folder`,"
                        " a `Path`), or say it unquoted"
                    )
    problems += [
        f"{shown(QUOTED)}: `{' '.join(key)}` quotes nothing any more: take its line out"
        for key in sorted(listed - seen)
    ]
    return problems


def string_literal(text, at):
    """The Rust string literal at `at`: its value and where it ends."""
    raw = re.match(r'r(#*)"', text[at:])
    if raw:
        hashes = raw.group(1)
        start = at + len(raw.group(0))
        end = text.index('"' + hashes, start)
        return text[start:end], end + 1 + len(hashes)
    if text[at] != '"':
        raise ValueError("not a string")
    out, i = [], at + 1
    while text[i] != '"':
        c = text[i]
        if c != "\\":
            out.append(c)
            i += 1
            continue
        nxt = text[i + 1]
        if nxt == "\n":
            # A line continued: the newline and the next line's leading
            # whitespace are not the string's.
            i += 2
            while text[i] in " \t\n\r":
                i += 1
            continue
        simple = {"n": "\n", "t": "\t", "r": "\r", "0": "\0", "\\": "\\", '"': '"', "'": "'"}
        if nxt in simple:
            out.append(simple[nxt])
            i += 2
        elif nxt == "u":
            close = text.index("}", i)
            out.append(chr(int(text[i + 3 : close], 16)))
            i = close + 1
        elif nxt == "x":
            out.append(chr(int(text[i + 2 : i + 4], 16)))
            i += 4
        else:
            raise ValueError(f"unknown escape \\{nxt}")
    return "".join(out), i + 1


def skip_blank(text, at):
    while True:
        while at < len(text) and text[at].isspace():
            at += 1
        if text.startswith("//", at):
            at = text.index("\n", at)
        else:
            return at


def calls(text):
    """Each `words!` call of `text`: its id, English, note, and whether it
    is a noun."""
    for match in CALL.finditer(text):
        # One in a comment (a doc's example) is no call.
        line_start = text.rfind("\n", 0, match.start()) + 1
        if "//" in text[line_start : match.start()]:
            continue
        at = skip_blank(text, match.end())
        ident = re.match(r"[a-z0-9_]+", text[at:])
        if not ident:
            continue
        at = skip_blank(text, at + ident.end())
        if text[at] != "=":
            continue
        at = skip_blank(text, at + 1)
        english, at = string_literal(text, at)
        # The rest, to the call's end, at its own depth: a note, a noun.
        note, depth, noun = "", 1, False
        while depth:
            c = text[at]
            if c == "," and depth == 1 and NOUN.match(text, at):
                noun = True
                at = NOUN.match(text, at).end()
                continue
            if c in "([{":
                depth += 1
            elif c in ")]}":
                depth -= 1
            elif c == '"' or (c == "r" and re.match(r'r#*"', text[at:])):
                _, at = string_literal(text, at)
                continue
            elif depth == 1 and re.match(r"note\s*=", text[at:]) and not text[at - 1].isalnum():
                at = skip_blank(text, at + text[at:].index("=") + 1)
                note, at = string_literal(text, at)
                continue
            at += 1
        yield ident.group(0), english, note, noun


def outside(path):
    """Whether `path` is in a plugin from outside Skwir
    (`crates/plugins-outside/`): its catalogs are its own, not Skwir's."""
    try:
        return path.relative_to(ROOT).parts[:2] == ("crates", "plugins-outside")
    except ValueError:
        return False


def crate_of(path):
    """The crate a source belongs to: its folder and package name."""
    for folder in path.parents:
        manifest = folder / "Cargo.toml"
        if manifest.is_file():
            name = re.search(r'^name\s*=\s*"([^"]+)"', manifest.read_text(), re.M)
            return folder, name.group(1) if name else folder.name
    raise ValueError(f"{path} is in no crate")


def shown(path):
    """`path` as a problem names it: from the repository's root."""
    try:
        return path.relative_to(ROOT)
    except ValueError:
        return path


def messages_of(source):
    """Each message of a catalog's `source`, by its id: whether it is a
    draft, a line of the `#` comment just above it (attached, as Fluent
    attaches it: no blank line between) saying `draft`. A message goes on
    over its indented and blank lines, and over every line while a
    placeable it opened is open, as the program's own index reads it
    (`crates/core/src/words/catalog.rs`)."""
    found, comment = {}, []
    open_message, depth, in_string, escaped = False, 0, False, False

    def braces(text):
        nonlocal depth, in_string, escaped
        for c in text:
            if in_string:
                if escaped:
                    escaped = False
                elif c == "\\":
                    escaped = True
                elif c == '"':
                    in_string = False
            elif c == "{":
                depth += 1
            elif c == "}":
                depth = max(depth - 1, 0)
            elif c == '"' and depth:
                in_string = True

    for line in source.splitlines():
        if open_message and (depth or line.startswith((" ", "\t")) or not line.strip()):
            braces(line)
            continue
        open_message, depth, in_string, escaped = False, 0, False, False
        if line.startswith("#"):
            # A message's own comment; a group's or the file's (`##`,
            # `###`) is no message's.
            comment = comment + [line[1:].strip()] if line == "#" or line.startswith("# ") else []
            continue
        message = MESSAGE.match(line)
        if message:
            found[message.group(1)] = "draft" in comment
            open_message = True
            braces(line[message.end() :])
        comment = []
    return found


def on_by_default(folder, name):
    """Whether Skwir comes with the crate `name` (in `folder`) on: the
    program's own crates always, a plugin's unless its plugin answers
    `enabled_by_default` with `false`. Raises where it answers anything but
    a literal, which this tool cannot read."""
    if not name.startswith(PLUGIN_CRATE):
        return True
    answers = []
    for path in sorted((folder / "src").rglob("*.rs")):
        text = path.read_text()
        for said in DEFAULT_SAID.finditer(text):
            line_start = text.rfind("\n", 0, said.start()) + 1
            if "//" in text[line_start : said.start()]:
                continue
            answer = DEFAULT_ANSWER.match(text, said.start())
            if not answer:
                raise ValueError(
                    f"{shown(path)}: `enabled_by_default` answers no literal `true` or `false`,"
                    " which `just words` reads to know whether its words are on by default"
                )
            answers.append(answer.group(1) == "true")
    # None said: the plugin's default, on.
    return all(answers)


# The message saying a language's name in itself, in core's catalogs.
SELF_NAME = "settings_language_self"
CORE = "skwir-core"


def own_name(found, names, language):
    """A language's name in itself (Français), as its catalog of core says
    it (`settings_language_self`): counted here, so that the program names
    each language without reading its catalogs. Its tag where that catalog
    says none."""
    for folder in found:
        if names.get(folder) != CORE:
            continue
        path = folder / "l10n" / f"{language}.ftl"
        if not path.exists():
            continue
        for line in path.read_text().splitlines():
            if line.startswith(f"{SELF_NAME} ="):
                value = line.split("=", 1)[1].strip()
                if value:
                    return value
    return language


def shares(found, names):
    """Each language a catalog is in, by its tag, sorted: of all the
    crates' messages, how many it translates and how many of those are
    reviewed, whether every message of the crates on by default is
    translated, and reviewed, and its name in itself. `found` holds each
    crate's messages by its folder, `names` its package name. Also the
    problems met."""
    problems, defaults = [], {}
    for folder in found:
        try:
            defaults[folder] = on_by_default(folder, names[folder])
        except ValueError as why:
            problems.append(str(why))
            defaults[folder] = True
    total = sum(len(messages) for messages in found.values())
    languages = sorted(
        {path.stem for folder in found for path in (folder / "l10n").glob("*.ftl") if path.stem != "en"}
    )
    out = []
    for language in languages:
        share = {
            "language": language,
            "messages": total,
            "translated": 0,
            "reviewed": 0,
            "default_whole": True,
            "default_translated": True,
            "name": own_name(found, names, language),
        }
        for folder, messages in sorted(found.items()):
            path = folder / "l10n" / f"{language}.ftl"
            said = messages_of(path.read_text()) if path.exists() else {}
            translated = [id_ for id_ in said if id_ in messages]
            reviewed = sum(1 for id_ in translated if not said[id_])
            share["translated"] += len(translated)
            share["reviewed"] += reviewed
            if reviewed < len(messages) and defaults[folder]:
                share["default_whole"] = False
            if len(translated) < len(messages) and defaults[folder]:
                share["default_translated"] = False
        out.append(share)
    return out, problems


def percent(share, key):
    """A share's `key` (translated, reviewed) in whole percents of all
    messages, rounded down: 100 only once every message is."""
    return share[key] * 100 // share["messages"] if share["messages"] else 0


def meets_bar(share):
    """Whether Automatic offers the language, reviewed, from now on
    (ADR-139, 13)."""
    return bool(share["messages"]) and share["reviewed"] * 100 >= OFFER_AT * share["messages"] and share["default_whole"]


def meets_draft_bar(share):
    """Whether Automatic speaks the language as a draft from now on
    (ADR-139, 13): translated as much as an offered one is reviewed."""
    return (
        bool(share["messages"])
        and share["translated"] * 100 >= OFFER_AT * share["messages"]
        and share["default_translated"]
    )


def under_floor(share):
    """Whether a release refuses the language, offered (ADR-139, 13)."""
    return share["reviewed"] * 100 < KEEP_AT * share["messages"]


DRAFT = "draft"


def read_offered(path):
    """The languages the record offers, in its order, each with whether
    it is offered as a draft; and the problems met."""
    if not path.exists():
        return {}, []
    record, problems = {}, []
    for line in path.read_text().splitlines():
        words = line.split()
        if not words or words[0].startswith("#"):
            continue
        if len(words) > 2 or (len(words) == 2 and words[1] != DRAFT):
            problems.append(f"{shown(path)}: `{line.strip()}`: a line is a tag, then `{DRAFT}` or nothing")
            continue
        record[words[0]] = len(words) == 2
    return record, problems


def write_offered(path, record):
    """The record again, its comments and order kept: each language in it
    as `record` says, the new ones last."""
    lines, written = [], set()
    for line in (path.read_text().splitlines() if path.exists() else []):
        words = line.split()
        if words and not words[0].startswith("#") and words[0] in record:
            tag = words[0]
            lines.append(f"{tag} {DRAFT}" if record[tag] else tag)
            written.add(tag)
        else:
            lines.append(line)
    for tag, draft in record.items():
        if tag not in written:
            lines.append(f"{tag} {DRAFT}" if draft else tag)
    path.write_text("\n".join(lines).rstrip("\n") + "\n")


def catalogs():
    """Each crate's messages, by its folder: id to (English, note,
    whether it is a noun)."""
    found, problems = {}, []
    for path in sorted((ROOT / "crates").rglob("*.rs")):
        relative = path.relative_to(ROOT)
        if "target" in relative.parts or path.name == "tests.rs" or "tests" in relative.parts:
            continue
        if outside(path):
            continue
        text = path.read_text()
        if "words!" not in text:
            continue
        folder, _ = crate_of(path)
        messages = found.setdefault(folder, {})
        for id_, english, note, noun in calls(text):
            if id_.startswith("test_"):
                continue
            kept = messages.get(id_)
            if kept and kept[0] != english:
                problems.append(f"{relative}: `{id_}` has two Englishes in its crate")
                continue
            messages[id_] = (english, note or (kept[1] if kept else ""), noun or bool(kept and kept[2]))
            parted = PARTED.search(english)
            if parted:
                problems.append(
                    f"{relative}: `{id_}` parts a number from its unit with a plain space"
                    f" ({parted.group(0)!r}), where a line may break: join them with a"
                    " no-break space, `\\u{a0}`"
                )
            spaced = SPACED_PERCENT.search(english)
            if spaced:
                problems.append(
                    f"{relative}: `{id_}` writes a space before a percent sign"
                    f" ({spaced.group(0)!r}): English writes it right after its number (42%)"
                )
    return {folder: messages for folder, messages in found.items() if messages}, problems


def indented(english):
    """`english` with its later lines indented four spaces deeper than the
    least of them, as Fluent reads it the same (the code's own indent is
    nothing a translator needs)."""
    lines = english.split("\n")
    rest = [line for line in lines[1:] if line.strip()]
    if not rest:
        return english
    least = min(len(line) - len(line.lstrip()) for line in rest)
    return "\n".join([lines[0]] + ["    " + line[least:] if line.strip() else "" for line in lines[1:]])


def ftl(messages):
    out = [HEADER]
    for id_ in sorted(messages):
        english, note, noun = messages[id_]
        english = indented(english)
        out.append("\n")
        for line in note.splitlines():
            out.append(f"# {line}\n")
        if noun:
            out.append(f"# {NOUN_LINE}\n")
        out.append(f"{id_} = {english}\n")
    return "".join(out)


CATALOGS_RS = ROOT / "crates" / "skwir" / "src" / "catalogs.rs"


OFFERS = {None: "Incomplete", True: "Draft", False: "Reviewed"}


def rust_string(text):
    """`text` as a Rust string literal."""
    return '"' + text.replace("\\", "\\\\").replace('"', '\\"') + '"'


def shipped(language_shares, offered):
    """The program's list of shipped catalogs, and each language's share
    (`offered` the languages Automatic speaks, each with whether it is a
    draft), as Rust."""
    lines = [
        "// Made from the crates' l10n folders by `just words` (tools/words/extract.py):",
        "// never edit by hand. Every catalog but English, compiled in (ADR-139, G),",
        "// and how much of each language they say (ADR-139, 13).",
        "",
        "#[rustfmt::skip]",
        "pub static CATALOGS: &[skwir_core::words::Shipped] = &[",
    ]
    for path in sorted((ROOT / "crates").glob("**/l10n/*.ftl")):
        if "target" in path.relative_to(ROOT).parts or path.stem == "en" or outside(path):
            continue
        _, domain = crate_of(path)
        relative = path.relative_to(CATALOGS_RS.parent.parent.parent)
        lines += [
            "    skwir_core::words::Shipped {",
            f'        domain: "{domain}",',
            f'        language: "{path.stem}",',
            f'        source: include_str!("../../{relative.as_posix()}"),',
            "    },",
        ]
    lines += [
        "];",
        "",
        "/// Of all the messages, how many each language's catalogs translate and",
        "/// how many of those are reviewed, whether Skwir offers it, as a draft",
        "/// or reviewed (`tools/words/offered`), and its name in itself.",
        "#[rustfmt::skip]",
        "pub static SHARES: &[skwir_core::words::Share] = &[",
    ]
    for share in language_shares:
        lines += [
            "    skwir_core::words::Share {",
            f'        language: "{share["language"]}",',
            f"        messages: {share['messages']:_},",
            f"        translated: {share['translated']:_},",
            f"        reviewed: {share['reviewed']:_},",
            f"        offered: skwir_core::words::Offer::{OFFERS[offered.get(share['language'])]},",
            f"        name: {rust_string(share['name'])},",
            "    },",
        ]
    lines.append("];")
    return "\n".join(lines) + "\n"


def said_share(share, offered):
    """A language's share, as `just words` tells it."""
    state = {None: "incomplete", True: "offered as a draft", False: "offered"}[offered.get(share["language"])]
    return (
        f"{share['language']}: {percent(share, 'translated')}% of {share['messages']} messages translated,"
        f" {percent(share, 'reviewed')}% reviewed; {state}"
    )


def main():
    flags = ("--check", "--release")
    unknown = [arg for arg in sys.argv[1:] if arg not in flags]
    if unknown:
        print(f"unknown argument {unknown[0]}: only --check or --release", file=sys.stderr)
        return 2
    release = "--release" in sys.argv[1:]
    check = release or "--check" in sys.argv[1:]
    found, problems = catalogs()
    names = {folder: crate_of(folder / "Cargo.toml")[1] for folder in found}
    language_shares, share_problems = shares(found, names)
    problems += share_problems
    record, record_problems = read_offered(OFFERED)
    problems += record_problems
    listed, listed_problems = read_quoted(QUOTED)
    problems += listed_problems + quoted_problems(found, listed)
    known = {share["language"] for share in language_shares}
    problems += [
        f"{shown(OFFERED)}: {tag} is no language a catalog is in" for tag in record if tag not in known
    ]
    # Each language moves up as it reaches a bar: in as a draft once
    # translated, its draft mark gone once reviewed; never down.
    offered = dict(record)
    moved = []
    for share in language_shares:
        tag = share["language"]
        if record.get(tag, True) and meets_bar(share):
            offered[tag] = False
            moved.append(f"{tag} offered from now on ({percent(share, 'reviewed')}% reviewed)")
        elif tag not in record and meets_draft_bar(share):
            offered[tag] = True
            moved.append(
                f"{tag} offered as a draft from now on ({percent(share, 'translated')}% translated,"
                f" {percent(share, 'reviewed')}% reviewed)"
            )
    if moved:
        if check:
            problems += [f"{said}, once `just words` writes it in {shown(OFFERED)}" for said in moved]
        else:
            write_offered(OFFERED, offered)
            for said in moved:
                print(f"{shown(OFFERED)}: {said}")
    for share in language_shares:
        tag = share["language"]
        if tag not in record:
            continue
        if not record[tag] and under_floor(share):
            said = (
                f"{tag} is offered with only {percent(share, 'reviewed')}% of its messages"
                f" reviewed, and a release offers none under {KEEP_AT}%: review more of it, or take it out"
                f" of {shown(OFFERED)}, which lists it as incomplete again"
            )
        elif record[tag] and not meets_draft_bar(share):
            said = (
                f"{tag} is offered as a draft with {percent(share, 'translated')}% of its messages translated"
                f" and {'all' if share['default_translated'] else 'not all'} of what is on by default, and a"
                f" release offers none short of {OFFER_AT}% and all: draft what it lacks, or take it out of"
                f" {shown(OFFERED)}, which lists it as incomplete again"
            )
        else:
            continue
        if release:
            problems.append(said)
        else:
            print(f"warning: {said}", file=sys.stderr)
    wanted_rs = shipped(language_shares, offered)
    now_rs = CATALOGS_RS.read_text() if CATALOGS_RS.exists() else None
    if now_rs != wanted_rs:
        if check:
            problems.append(f"{CATALOGS_RS.relative_to(ROOT)} is not what the catalogs are: run `just words`")
        else:
            CATALOGS_RS.write_text(wanted_rs)
            print(f"{CATALOGS_RS.relative_to(ROOT)}: made again")
            for share in language_shares:
                print(f"  {said_share(share, offered)}")
    for folder, messages in sorted(found.items()):
        path = folder / "l10n" / "en.ftl"
        wanted = ftl(messages)
        now = path.read_text() if path.exists() else None
        if now == wanted:
            continue
        if check:
            problems.append(f"{path.relative_to(ROOT)} is not what the code says: run `just words`")
        else:
            path.parent.mkdir(exist_ok=True)
            path.write_text(wanted)
            print(f"{path.relative_to(ROOT)}: {len(messages)} messages")
    # A catalog whose crate says no message any more; a plugin's from
    # outside is written by its author, never made from code.
    for path in sorted((ROOT / "crates").glob("**/l10n/en.ftl")):
        if path.parents[1] not in found and not outside(path):
            problems.append(f"{path.relative_to(ROOT)}: its crate says no message")
    for problem in problems:
        print(problem, file=sys.stderr)
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
