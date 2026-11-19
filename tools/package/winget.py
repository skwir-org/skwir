#!/usr/bin/env python3
"""The winget manifests of a release (ADR-142, 3; ADR-074): what
microsoft/winget-pkgs takes, so `winget upgrade` and the programs built on it
(UniGetUI) update a copy of Skwir installed by its installer, by hand or by
winget. Made from the release's bundle, which `release-build` made and
`release-verify` checked: its version, date and the installer's SHA-256 from
its manifest, the installer at the release's own address.

    tools/package/winget.py target/releases/v0.6.0 [--out FOLDER]

writes, in FOLDER (by default the bundle's sibling `v0.6.0-winget`),
`manifests/s/Skwir/Skwir/0.6.0/` (winget-pkgs' layout: the identifier's
first letter, then its parts) with its three files: the version, the
installer and the English description. Submitting them is the user's, as
publishing the AUR's bases is (`docs/RELEASING.md`): a pull request to
winget-pkgs with these files, by hand or with `wingetcreate submit` or
`komac submit`. Nothing here reaches the network.
"""

import argparse
import json
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
IDENTIFIER = "Skwir.Skwir"
SCHEMA = "1.10.0"
REPOSITORY = "https://github.com/skwir-org/skwir"


def quoted(value):
    """A YAML scalar: plain where it is safe, else double-quoted."""
    text = str(value)
    if re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9 ._/:+-]*", text) and not text.endswith(":") and ": " not in text:
        return text
    return json.dumps(text, ensure_ascii=False)


def document(kind, fields):
    """One manifest file: its schema's line, then its fields in order."""
    lines = [f"# yaml-language-server: $schema=https://aka.ms/winget-manifest.{kind}.{SCHEMA}.schema.json", ""]
    for key, value in fields:
        if isinstance(value, list):
            lines.append(f"{key}:")
            for item in value:
                if isinstance(item, list):
                    first, *rest = item
                    lines.append(f"- {first[0]}: {quoted(first[1])}")
                    lines.extend(f"  {k}: {quoted(v)}" for k, v in rest)
                else:
                    lines.append(f"- {quoted(item)}")
        else:
            lines.append(f"{key}: {quoted(value)}")
    return "\n".join(lines) + "\n"


def upgrade_code():
    """The installer's upgrade code, as skwir.wxs keeps it from one release to
    the next: what winget matches a copy installed by hand with."""
    wxs = (ROOT / "tools/package/windows/skwir.wxs").read_text()
    code = re.search(r'UpgradeCode="([0-9a-fA-F-]{36})"', wxs).group(1)
    return "{" + code.upper() + "}"


def summary():
    """What Skwir is, in a line, as the desktop's software centres say it."""
    metainfo = (ROOT / "tools/package/linux/org.skwir.skwir.metainfo.xml").read_text()
    return re.search(r"<summary>([^<]+)</summary>", metainfo).group(1)


def manifests(bundle):
    """The three files, by name, of the release in `bundle`."""
    manifest = json.loads((bundle / "manifest.json").read_text())
    version, tag, date = manifest["version"], manifest["tag"], manifest["date"]
    installer = f"skwir-{version}-windows-x86_64.msi"
    sha256 = manifest["assets"][installer]["sha256"].upper()
    common = [("PackageIdentifier", IDENTIFIER), ("PackageVersion", version)]
    return {
        f"{IDENTIFIER}.yaml": document("version", [
            *common,
            ("DefaultLocale", "en-US"),
            ("ManifestType", "version"),
            ("ManifestVersion", SCHEMA),
        ]),
        f"{IDENTIFIER}.installer.yaml": document("installer", [
            *common,
            ("InstallerType", "wix"),
            ("Scope", "user"),
            ("InstallModes", ["interactive", "silent", "silentWithProgress"]),
            ("UpgradeBehavior", "install"),
            ("ReleaseDate", date),
            ("AppsAndFeaturesEntries", [[("DisplayName", "skwir"), ("Publisher", "skwir"),
                                         ("UpgradeCode", upgrade_code())]]),
            ("Installers", [[("Architecture", "x64"),
                             ("InstallerUrl", f"{REPOSITORY}/releases/download/{tag}/{installer}"),
                             ("InstallerSha256", sha256)]]),
            ("ManifestType", "installer"),
            ("ManifestVersion", SCHEMA),
        ]),
        f"{IDENTIFIER}.locale.en-US.yaml": document("defaultLocale", [
            *common,
            ("PackageLocale", "en-US"),
            ("Publisher", "The Skwir authors"),
            ("PublisherUrl", "https://skwir.org"),
            ("PublisherSupportUrl", "https://skwir.org/issues"),
            ("PackageName", "Skwir"),
            ("PackageUrl", "https://skwir.org"),
            ("License", "Apache-2.0"),
            ("LicenseUrl", f"{REPOSITORY}/blob/{tag}/LICENSE"),
            ("ShortDescription", summary()),
            ("Description", "A file manager for humans and agents: previews, servers, phones and cameras as "
                            "folders, search and tags, plans an agent proposes for your review, and Undo. "
                            "The Windows build is one to try: made on Linux, tried under Wine."),
            ("Tags", ["file-manager", "files", "explorer", "preview", "tags"]),
            ("ReleaseNotesUrl", f"https://skwir.org/releases/{tag}"),
            ("ManifestType", "defaultLocale"),
            ("ManifestVersion", SCHEMA),
        ]),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("bundle", type=pathlib.Path)
    parser.add_argument("--out", type=pathlib.Path)
    args = parser.parse_args()
    bundle = args.bundle.resolve()
    if not (bundle / "manifest.json").is_file():
        sys.exit(f"{bundle}: no release bundle (run `just release-build <tag>` first)")
    files = manifests(bundle)
    version = json.loads((bundle / "manifest.json").read_text())["version"]
    out = (args.out or bundle.with_name(bundle.name + "-winget")).resolve()
    folder = out / "manifests" / IDENTIFIER[0].lower() / IDENTIFIER.replace(".", "/") / version
    folder.mkdir(parents=True, exist_ok=True)
    for name, text in files.items():
        (folder / name).write_text(text)
        print(f"made: {folder / name}")


if __name__ == "__main__":
    main()
