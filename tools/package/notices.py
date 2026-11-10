#!/usr/bin/env python3
"""The third-party notices each package ships: every crate its programs
link, under the licence it is used under, with that licence's text as the
crate carries it (its copyright lines included). The MIT, BSD, ISC and
Zlib licences ask for their notices in a program's distribution.

    tools/package/notices.py            # into tools/package/notices/<package>/THIRD-PARTY-NOTICES
    tools/package/notices.py --check    # fail if those are not what the crates say now

They are made here and kept with the source (ADR-063), which the packages
copy: they depend only on Cargo.lock and the programs each package ships,
and cargo-about reads every platform's crates, which a package build that
fetched only its own has not. A dependency added or updated, or a program
shipped in another package, makes them again (`just notices`);
`just notices-check`, in release-check, refuses stale ones.

Needs cargo-about (in the dev shell; `cargo install --locked --features
cli cargo-about` elsewhere: its program is behind `cli` since 0.9). It
fetches every platform's crates, then reads them offline: online, cargo-about
would look for a licence a crate leaves out on the network, and the notices
would depend on what it found that day. Which programs a package ships is
tools/package/programs: the ones shipped apart (tools/package/optional) each
a package of its own (`skwir-<name>-worker` in `skwir-<name>`), the others
`skwir`'s, as Debian's, Fedora's and the AppImage's packages hold them.
Arch's keeps apart only the packages its PKGBUILD names, so its `skwir`
has notices of its own (arch/skwir). Windows' package holds the programs
tools/package/windows/programs lists, built for Windows, whose crates
differ (windows/skwir, ADR-074); macOS's those of
tools/package/macos/programs, built for Apple silicon (macos/skwir,
ADR-075).
"""

import functools
import json
import pathlib
import re
import subprocess
import sys
import textwrap

ROOT = pathlib.Path(__file__).resolve().parents[2]
CONFIG = ROOT / "tools/package/about.toml"
OUT = ROOT / "tools/package/notices"
PKGBUILD = ROOT / "tools/package/aur/PKGBUILD"
NAME = "THIRD-PARTY-NOTICES"
# What Windows' and macOS's packages are built for (tools/package/windows.sh,
# tools/package/macos.sh).
WINDOWS = "x86_64-pc-windows-gnu"
MACOS = "aarch64-apple-darwin"


def lines(name):
    text = (ROOT / "tools/package" / name).read_text()
    return [l.strip() for l in text.splitlines() if l.strip() and not l.startswith("#")]


def packages(apart):
    out = {"skwir": []}
    for program in lines("programs"):
        if program in apart:
            name = program.removesuffix("-worker")
            out.setdefault(name, []).append(program)
        else:
            out["skwir"].append(program)
    return out


def arch_apart():
    """The programs Arch's packages hold apart from `skwir`: a
    `skwir-<name>-worker` for each `skwir-<name>` its PKGBUILD's pkgname
    lists."""
    names = re.search(r"^pkgname=\(([^)]*)\)$", PKGBUILD.read_text(), re.M)
    if not names:
        sys.exit(f"no pkgname in {PKGBUILD}")
    apart = {f"{name}-worker" for name in names[1].split() if name != "skwir"}
    if missing := apart - set(lines("programs")):
        sys.exit(f"{PKGBUILD} names packages of no program: {', '.join(sorted(missing))}")
    return apart


def files():
    """Each notices file, under OUT, and the package, programs and target
    it is for (`None`: about.toml's, Linux's): the packages Debian, Fedora
    and the AppImage ship, then those of Arch's that hold other programs,
    under arch/, then Windows' and macOS's packages, under windows/ and
    macos/."""
    split = packages(set(lines("optional")))
    out = {pathlib.Path(name) / NAME: (name, programs, None) for name, programs in split.items()}
    for name, programs in packages(arch_apart()).items():
        if split.get(name) != programs:
            out[pathlib.Path("arch") / name / NAME] = (name, programs, None)
    out[pathlib.Path("windows/skwir") / NAME] = (
        "skwir for Windows", lines("windows/programs"), WINDOWS)
    out[pathlib.Path("macos/skwir") / NAME] = (
        "skwir for macOS", lines("macos/programs"), MACOS)
    return out


def manifest(program):
    """The crate a program is built from: `skwir` is crates/skwir, a
    helper `skwir-x-worker` crates/x-worker."""
    folder = "skwir" if program == "skwir" else program.removeprefix("skwir-")
    path = ROOT / "crates" / folder / "Cargo.toml"
    if not path.exists():
        sys.exit(f"no crate for {program}: {path}")
    return path


@functools.cache
def licences(program, target=None):
    """cargo-about's reading of what `program` links, built for `target`
    (about.toml's where `None`): (id, name, text) -> the crates that use
    it, as `name version`."""
    run = subprocess.run(
        [
            "cargo", "about", "generate", "--offline", "--fail",
            "--format", "json", "-c", str(CONFIG), "-m", str(manifest(program)),
            *(["--target", target] if target else []),
        ],
        cwd=ROOT, capture_output=True, text=True,
    )
    if run.returncode != 0:
        sys.exit(f"cargo about for {program}:\n{run.stderr}")
    found = {}
    for licence in json.loads(run.stdout)["licenses"]:
        # A crate's licence file may end its lines as Windows does.
        text = licence["text"].replace("\r\n", "\n").replace("\r", "\n").strip()
        key = (licence["id"], licence["name"], text)
        crates = {f'{u["crate"]["name"]} {u["crate"]["version"]}' for u in licence["used_by"]}
        found.setdefault(key, set()).update(crates)
    return found


# What skwir's program carries that is no crate: the code themes compiled
# into skwir-platform (crates/platform/assets/code-themes, ADR-071), each
# under the licence beside it, and the palettes whose colors it wears.
CARRIED = ROOT / "crates/platform/assets/code-themes"
CARRIED_THEMES = [
    ("Rosé Pine's code themes (rose-pine.tmTheme, rose-pine-dawn.tmTheme)",
     "MIT License (MIT)", "rose-pine.LICENSE"),
    ("Tokyo Night's code themes (tokyonight_night.tmTheme, tokyonight_day.tmTheme), "
     "by Folke Lemaitre, changed by Skwir as each file's first comment says",
     "Apache License 2.0 (Apache-2.0)", "tokyonight.LICENSE"),
    ("Everforest's code themes (everforest-dark.tmTheme, everforest-light.tmTheme), "
     "from everforest-textmate",
     "MIT License (MIT)", "everforest-textmate.LICENSE"),
]
PALETTES = (
    "Skwir's palettes wear the colors of Solarized (Ethan Schoonover, MIT), "
    "Nord (Sven Greb, MIT), Gruvbox (Pavel Pertsev, MIT), Catppuccin (MIT), "
    "Dracula (Dracula Theme, MIT), Rosé Pine (MIT), Tokyo Night (Enkia, MIT; "
    "Folke Lemaitre's tokyonight.nvim, Apache-2.0) and Everforest "
    "(Sainnhe Park, MIT), as their makers publish them."
)


def carried():
    """The notices of what the skwir program carries that is no crate."""
    rule = "-" * 72
    out = [rule, "Carried in skwir, not crates", "",
           *textwrap.wrap(PALETTES, 72, break_on_hyphens=False), ""]
    for what, name, licence in CARRIED_THEMES:
        out += [rule, name, "",
                *textwrap.wrap("Used by: " + what, 72, break_on_hyphens=False), "",
                (CARRIED / licence).read_text().rstrip("\n"), ""]
    return out


def katex_fonts():
    """KaTeX's fonts, which ratex-katex-fonts carries into the math helper
    (ADR-129): under the SIL Open Font License, not the crate's own MIT, as
    the notice beside them says; both are the crate's version's."""
    run = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--locked", "--offline"],
        cwd=ROOT, capture_output=True, text=True, check=True,
    )
    crate = next(p for p in json.loads(run.stdout)["packages"] if p["name"] == "ratex-katex-fonts")
    fonts = pathlib.Path(crate["manifest_path"]).parent / "fonts"
    rule = "-" * 72
    return [rule, "SIL Open Font License 1.1 (OFL-1.1)", "",
            *textwrap.wrap(f"Used by: KaTeX's fonts (KaTeX_*.ttf), carried by "
                           f"ratex-katex-fonts {crate['version']}", 72, break_on_hyphens=False), "",
            (fonts / "FONT_NOTICE.txt").read_text().rstrip("\n"), "",
            (fonts / "OFL.txt").read_text().rstrip("\n"), ""]


def notices(package, programs, target):
    merged = {}
    for program in programs:
        for key, crates in licences(program, target).items():
            merged.setdefault(key, set()).update(crates)
    ours = {c for crates in merged.values() for c in crates if c.split()[0].startswith("skwir")}
    out = [
        f"Third-party software in {package}",
        "",
        *textwrap.wrap(
            f"The programs of this package ({', '.join(programs)}) are built from "
            "skwir's own code, under the package's licence, and the crates below, "
            "each under the licence it is listed with, whose text follows.",
            72,
            break_on_hyphens=False,
        ),
        "",
    ]
    rule = "-" * 72
    for (ident, name, text), crates in sorted(merged.items(), key=lambda kv: (kv[0][0], sorted(kv[1]))):
        used = sorted(crates - ours)
        if not used:
            continue
        out += [rule, f"{name} ({ident})", "", "Used by: " + ", ".join(used), "", text, ""]
    if "skwir" in programs:
        out += carried()
    if "skwir-math-worker" in programs:
        out += katex_fonts()
    return "\n".join(out) + "\n"


def main():
    check = sys.argv[1:] == ["--check"]
    if sys.argv[1:] and not check:
        sys.exit(f"usage: {sys.argv[0]} [--check]")
    # Every platform's crates, which cargo-about's graph names, read offline
    # below; nothing is downloaded once they are there.
    subprocess.run(["cargo", "fetch", "--locked", "--quiet"], cwd=ROOT, check=True)
    wanted = files()
    made = {path: notices(*wanted[path]) for path in wanted}
    kept = {p.relative_to(OUT) for p in OUT.rglob(NAME)}
    if check:
        stale = [p for p, text in made.items()
                 if p not in kept or (OUT / p).read_bytes() != text.encode()]
        if stale or kept - made.keys():
            listed = ", ".join(str(p) for p in sorted([*stale, *(kept - made.keys())]))
            sys.exit(f"The notices in {OUT.relative_to(ROOT)} are not the crates' now "
                     f"({listed}): run `just notices`")
        print(f"{OUT.relative_to(ROOT)}: {len(made)} notices, current")
        return
    for path, text in made.items():
        (OUT / path).parent.mkdir(parents=True, exist_ok=True)
        (OUT / path).write_bytes(text.encode())
        print(f"{(OUT / path).relative_to(ROOT)}: {len(wanted[path][1])} programs")
    # A package no longer made, or no longer Arch's own.
    for path in kept - made.keys():
        (OUT / path).unlink()
        if not any((OUT / path).parent.iterdir()):
            (OUT / path).parent.rmdir()
        print(f"{(OUT / path).relative_to(ROOT)}: removed")


if __name__ == "__main__":
    main()
