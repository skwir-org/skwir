#!/usr/bin/env python3
"""The types Skwir Preview opens (ADR-151, 8): every type of a kind Skwir
previews, as the desktop's type database groups types into kinds, listed
from the database Skwir carries (vendor/shared-mime-info) and never
written by hand. Its desktop entry,
tools/package/linux/org.skwir.skwir.view.desktop, holds them on its
`MimeType=` line.

    tools/package/viewer_types.py                  # the line, printed
    tools/package/viewer_types.py --check FILE     # status 1 where FILE's line is another
    tools/package/viewer_types.py --write FILE     # FILE's line made again

A type's kinds are read as Skwir reads them, `classify` in
crates/platform/src/mime.rs (`mime::kinds`, `mime::is_kind`): its name
unaliased, then the kind its generic icon says (the type's own
`<generic-icon>`, else `<top-level>-x-generic`, as xdg-mime's
`lookup_generic_icon_name` answers), and text where `text/plain` is in
its lineage (`mime::lineage`: the type, what it is a sub-class of, theirs
in turn, and `text/plain` for a type whose top level is `text`). Which
generic icon says which kind is read from `classify` itself, so the two
never part; a `classify` that no longer reads kinds so is refused rather
than mirrored wrong. A type is listed by the name the database gives it:
the desktop unaliases a desktop entry's types as it unaliases a file's.
Of them, those a desktop entry names are listed (`ENTRY_MEDIA_TYPES`):
not message/news, nor x-epoc/x-sisx-app.
"""

import argparse
import pathlib
import re
import sys
import xml.etree.ElementTree as ET

ROOT = pathlib.Path(__file__).resolve().parents[2]
DATABASE = ROOT / "vendor/shared-mime-info/freedesktop.org.xml.in"
CLASSIFY = ROOT / "crates/platform/src/mime.rs"
NS = "{http://www.freedesktop.org/standards/shared-mime-info}"
# The kinds Skwir previews (ADR-151, 8), as `mime::Kind` names them: all
# but folders and programs.
PREVIEWED = ("Image", "Video", "Audio", "Document", "Spreadsheet", "Presentation", "Text", "Archive",
             "Font")
# What a desktop entry names, as desktop-file-validate takes it without a
# word (desktop-file-utils 0.28, mimeutils.c, `is_valid_media_type`): a
# type under a registered discrete media type or under one freedesktop.org
# names, or one of the exceptions it makes of a composite type (a mail
# saved in a file). Of the others it says that they make no sense in a
# desktop entry (message/news) or that their media type is discouraged
# (x-epoc/…), each warning repeating the whole line.
ENTRY_MEDIA_TYPES = ("application", "audio", "font", "image", "model", "text", "video",
                     "inode", "x-content", "x-scheme-handler", "chemical")
ENTRY_EXCEPTIONS = ("message/rfc822", "multipart/related")
KEY = "MimeType="


def icon_kinds(source=CLASSIFY):
    """The kind each generic icon says, as `classify` matches them
    (`"image-x-generic" => Some(Kind::Image)`), and whether it still reads
    text as descent from `text/plain`."""
    text = source.read_text()
    body = re.search(r"\nfn classify\(.*?\n}\n", text, re.S)
    if body is None:
        sys.exit(f"{source}: no `fn classify` read: the kinds are read elsewhere now")
    body = body[0]
    arms = dict(re.findall(r'"([a-z-]+)" => Some\(Kind::(\w+)\)', body))
    if 'generic_icon_name()' not in body or not arms:
        sys.exit(f"{source}: `classify` no longer reads a kind from the generic icon")
    if not re.search(r'lineage\(&mime\)\.iter\(\)\.any\(\|m\| m\.as_str\(\) == "text/plain"\)', body):
        sys.exit(f"{source}: `classify` no longer reads text as descent from text/plain")
    missing = [kind for kind in PREVIEWED if kind != "Text" and kind not in arms.values()]
    if missing:
        sys.exit(f"{source}: no generic icon says {', '.join(missing)}")
    return arms


class Database:
    """What `classify` asks of the database, read from its source as
    update-mime-database compiles it: each type's generic icon
    (`generic-icons`), what it is a sub-class of (`subclasses`, the names
    as written) and its other names (`aliases`)."""

    def __init__(self, path=DATABASE):
        self.types, self.icons, self.parents, self.aliases = [], {}, {}, {}
        for element in ET.parse(path).getroot().iter(NS + "mime-type"):
            name = element.get("type")
            self.types.append(name)
            icons = [icon.get("name") for icon in element.iter(NS + "generic-icon")]
            if icons:
                # xdg-mime compares types parsed, whatever their case.
                self.icons[name.lower()] = icons[0]
            self.parents[name] = [parent.get("type") for parent in element.iter(NS + "sub-class-of")]
            for alias in element.iter(NS + "alias"):
                self.aliases[alias.get("type").lower()] = name

    def unalias(self, name):
        return self.aliases.get(name.lower(), name)

    def generic_icon(self, name):
        return self.icons.get(name.lower(), f"{name.split('/')[0]}-x-generic")

    def lineage(self, name):
        """`mime::lineage`: the type, then each one's parents, breadth
        first, then what its own top level implies."""
        out = [self.unalias(name)]
        at = 0
        while at < len(out):
            for parent in self.parents.get(out[at], []):
                if parent not in out:
                    out.append(parent)
            at += 1
        top = name.split("/")[0]
        for implied in ("text/plain" if top == "text" else None,
                        "application/octet-stream" if top != "inode" else None):
            if implied and implied not in out:
                out.append(implied)
        return out

    def kinds(self, name, by_icon):
        """`classify`, on Linux (where `system::kinds` adds nothing)."""
        if name == "inode/directory":
            return {"Folder"}
        name = self.unalias(name)
        found = set()
        if (kind := by_icon.get(self.generic_icon(name))) is not None:
            found.add(kind)
        if "text/plain" in self.lineage(name):
            found.add("Text")
        return found


def named_in_an_entry(name):
    return name.split("/")[0] in ENTRY_MEDIA_TYPES or name in ENTRY_EXCEPTIONS


def viewed(database=None, by_icon=None):
    """Every type of a kind Skwir previews that a desktop entry names,
    sorted."""
    database = database or Database()
    by_icon = by_icon or icon_kinds()
    return sorted(name for name in database.types
                  if database.kinds(name, by_icon) & set(PREVIEWED) and named_in_an_entry(name))


def line(types):
    return KEY + "".join(f"{name};" for name in types)


def line_of(path):
    """The desktop entry's `MimeType=` line, or None."""
    for text in path.read_text().splitlines():
        if text.startswith(KEY):
            return text
    return None


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    group = parser.add_mutually_exclusive_group()
    group.add_argument("--check", type=pathlib.Path, metavar="FILE")
    group.add_argument("--write", type=pathlib.Path, metavar="FILE")
    args = parser.parse_args()
    made = line(viewed())
    if args.check:
        if line_of(args.check) != made:
            sys.exit(f"{args.check}: its {KEY} line is not the types of the kinds Skwir previews, "
                     f"as {DATABASE.relative_to(ROOT)} groups them: run "
                     f"tools/package/viewer_types.py --write {args.check}")
        print(f"ok: {args.check} opens the {made.count(';')} types of the kinds Skwir previews")
    elif args.write:
        text = args.write.read_text()
        if line_of(args.write) is None:
            sys.exit(f"{args.write}: no {KEY} line to make again")
        lines = [made if row.startswith(KEY) else row for row in text.splitlines()]
        args.write.write_text("\n".join(lines) + "\n")
        print(f"{args.write}: {made.count(';')} types")
    else:
        print(made)


if __name__ == "__main__":
    main()
