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
has notices of its own (arch/skwir).
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
    """Each notices file, under OUT, and the package and programs it is
    for: the packages Debian, Fedora and the AppImage ship, then those of
    Arch's that hold other programs, under arch/."""
    split = packages(set(lines("optional")))
    out = {pathlib.Path(name) / NAME: (name, programs) for name, programs in split.items()}
    for name, programs in packages(arch_apart()).items():
        if split.get(name) != programs:
            out[pathlib.Path("arch") / name / NAME] = (name, programs)
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
def licences(program):
    """cargo-about's reading of what `program` links: (id, name, text) ->
    the crates that use it, as `name version`."""
    run = subprocess.run(
        [
            "cargo", "about", "generate", "--offline", "--fail",
            "--format", "json", "-c", str(CONFIG), "-m", str(manifest(program)),
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


def notices(package, programs):
    merged = {}
    for program in programs:
        for key, crates in licences(program).items():
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
    return "\n".join(out) + "\n"


def main():
    check = sys.argv[1:] == ["--check"]
    if sys.argv[1:] and not check:
        sys.exit(f"usage: {sys.argv[0]} [--check]")
    # Every platform's crates, which cargo-about's graph names, read offline
    # below; nothing is downloaded once they are there.
    subprocess.run(["cargo", "fetch", "--locked", "--quiet"], cwd=ROOT, check=True)
    wanted = files()
    made = {path: notices(name, programs) for path, (name, programs) in wanted.items()}
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
