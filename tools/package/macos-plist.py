#!/usr/bin/env python3
"""Write the macOS bundle's identity, viewer claim and offered languages."""

import argparse
from pathlib import Path
import plistlib
import re


def write(app: Path, version: str) -> None:
    match = re.fullmatch(r"(\d+\.\d+\.\d+)(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?", version)
    if not match:
        raise ValueError("version must be a three-part release version, optionally with a suffix")
    offered = Path(__file__).resolve().parents[2] / "tools/words/offered"
    languages = ["en", *sorted({
        line.split()[0] for line in offered.read_text().splitlines()
        if line.strip() and not line.startswith("#") and line.split()[0] != "en"
    })]
    contents = app / "Contents"
    contents.mkdir(parents=True, exist_ok=True)
    info = {
        "CFBundleName": "skwir",
        "CFBundleDisplayName": "skwir",
        "CFBundleIdentifier": "org.skwir.skwir",
        "CFBundleDevelopmentRegion": "en",
        "CFBundleLocalizations": languages,
        "CFBundleExecutable": "skwir",
        "CFBundleIconFile": "skwir",
        "CFBundlePackageType": "APPL",
        "CFBundleShortVersionString": version,
        "CFBundleVersion": match[1],
        "LSMinimumSystemVersion": "12.0",
        "NSHighResolutionCapable": True,
        # Sandboxed senders may reveal any item. Rank None declares the
        # ability without offering Skwir as the default app for all files.
        "CFBundleDocumentTypes": [{
            "CFBundleTypeName": "Filesystem item",
            "LSItemContentTypes": ["public.item"],
            "CFBundleTypeRole": "Viewer",
            "LSHandlerRank": "None",
        }],
    }
    (contents / "Info.plist").write_bytes(plistlib.dumps(info))
    for language in languages:
        folder = contents / "Resources" / f"{language}.lproj"
        folder.mkdir(parents=True, exist_ok=True)
        (folder / "InfoPlist.strings").write_text(
            "/* Skwir's name is the same in every language. */\n"
            'CFBundleName = "skwir";\nCFBundleDisplayName = "skwir";\n'
        )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("app", type=Path, help="path to the .app bundle")
    parser.add_argument("version", help="release version, including any prerelease suffix")
    args = parser.parse_args()
    try:
        write(args.app, args.version)
    except ValueError as error:
        parser.error(str(error))


if __name__ == "__main__":
    main()
