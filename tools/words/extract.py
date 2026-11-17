#!/usr/bin/env python3
"""Each crate's English catalog, made from its code (ADR-139, 2).

Every `words!(id = "English", …, note = "…")` of a crate's sources is a
message of that crate: written to `<crate>/l10n/en.ftl`, sorted by id, its
note as a comment above it. Never edited by hand.

    tools/words/extract.py           write every crate's en.ftl
    tools/words/extract.py --check   fail where one is not what the code says

It also writes the program's list of shipped catalogs,
crates/skwir/src/catalogs.rs: every crate's `l10n/<language>.ftl` but
English, compiled in (ADR-139, G).

Refused, in either mode: an id with two Englishes in one crate. Messages
of tests (in a tests.rs or a tests/ folder, or whose id begins with
`test_`) are not a crate's.
"""

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
HEADER = "# Made from the code by `just words` (tools/words/extract.py): never edit by hand.\n"
CALL = re.compile(r"\bwords!\s*\(")


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
    """Each `words!` call of `text`: its id, English and note."""
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
        # The rest, to the call's end, at its own depth: a note.
        note, depth = "", 1
        while depth:
            c = text[at]
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
        yield ident.group(0), english, note


def crate_of(path):
    """The crate a source belongs to: its folder and package name."""
    for folder in path.parents:
        manifest = folder / "Cargo.toml"
        if manifest.is_file():
            name = re.search(r'^name\s*=\s*"([^"]+)"', manifest.read_text(), re.M)
            return folder, name.group(1) if name else folder.name
    raise ValueError(f"{path} is in no crate")


def catalogs():
    """Each crate's messages, by its folder: id to (English, note)."""
    found, problems = {}, []
    for path in sorted((ROOT / "crates").rglob("*.rs")):
        relative = path.relative_to(ROOT)
        if "target" in relative.parts or path.name == "tests.rs" or "tests" in relative.parts:
            continue
        text = path.read_text()
        if "words!" not in text:
            continue
        folder, _ = crate_of(path)
        messages = found.setdefault(folder, {})
        for id_, english, note in calls(text):
            if id_.startswith("test_"):
                continue
            kept = messages.get(id_)
            if kept and kept[0] != english:
                problems.append(f"{relative}: `{id_}` has two Englishes in its crate")
                continue
            messages[id_] = (english, note or (kept[1] if kept else ""))
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
        english, note = messages[id_]
        english = indented(english)
        out.append("\n")
        for line in note.splitlines():
            out.append(f"# {line}\n")
        out.append(f"{id_} = {english}\n")
    return "".join(out)


CATALOGS_RS = ROOT / "crates" / "skwir" / "src" / "catalogs.rs"


def shipped():
    """The program's list of shipped catalogs, as Rust."""
    lines = [
        "// Made from the crates' l10n folders by `just words` (tools/words/extract.py):",
        "// never edit by hand. Every catalog but English, compiled in (ADR-139, G).",
        "",
        "#[rustfmt::skip]",
        "pub static CATALOGS: &[skwir_core::words::Shipped] = &[",
    ]
    for path in sorted((ROOT / "crates").glob("**/l10n/*.ftl")):
        if "target" in path.relative_to(ROOT).parts or path.stem == "en":
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
    lines.append("];")
    return "\n".join(lines) + "\n"


def main():
    unknown = [arg for arg in sys.argv[1:] if arg != "--check"]
    if unknown:
        print(f"unknown argument {unknown[0]}: only --check", file=sys.stderr)
        return 2
    check = "--check" in sys.argv[1:]
    found, problems = catalogs()
    wanted_rs = shipped()
    now_rs = CATALOGS_RS.read_text() if CATALOGS_RS.exists() else None
    if now_rs != wanted_rs:
        if check:
            problems.append(f"{CATALOGS_RS.relative_to(ROOT)} is not what the catalogs are: run `just words`")
        else:
            CATALOGS_RS.write_text(wanted_rs)
            print(f"{CATALOGS_RS.relative_to(ROOT)}: made again")
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
    # A catalog whose crate says no message any more.
    for path in sorted((ROOT / "crates").glob("**/l10n/en.ftl")):
        if path.parents[1] not in found:
            problems.append(f"{path.relative_to(ROOT)}: its crate says no message")
    for problem in problems:
        print(problem, file=sys.stderr)
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
