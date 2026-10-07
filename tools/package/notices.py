#!/usr/bin/env python3
"""The third-party notices each package ships: every crate its programs
link, under the licence it is used under, with that licence's text as the
crate carries it (its copyright lines included). The MIT, BSD, ISC and
Zlib licences ask for their notices in a program's distribution.

    tools/package/notices.py            # into target/notices/<package>/THIRD-PARTY-NOTICES
    tools/package/notices.py a b        # programs a and b apart, the rest bibli's

Needs cargo-about (in the dev shell; `cargo install --locked cargo-about`
elsewhere) and the crates fetched: it reads them offline, as the build
left them. Which programs a package ships is tools/package/programs: the
ones shipped apart (the arguments, else tools/package/optional) each a
package of its own (`bibli-<name>-worker` in `bibli-<name>`), the others
`bibli`'s.
"""

import json
import pathlib
import subprocess
import sys
import textwrap

ROOT = pathlib.Path(__file__).resolve().parents[2]
CONFIG = ROOT / "tools/package/about.toml"
OUT = ROOT / "target/notices"


def lines(name):
    text = (ROOT / "tools/package" / name).read_text()
    return [l.strip() for l in text.splitlines() if l.strip() and not l.startswith("#")]


def packages(apart):
    out = {"bibli": []}
    for program in lines("programs"):
        if program in apart:
            name = program.removesuffix("-worker")
            out.setdefault(name, []).append(program)
        else:
            out["bibli"].append(program)
    return out


def manifest(program):
    """The crate a program is built from: `bibli` is crates/bibli, a
    helper `bibli-x-worker` crates/x-worker."""
    folder = "bibli" if program == "bibli" else program.removeprefix("bibli-")
    path = ROOT / "crates" / folder / "Cargo.toml"
    if not path.exists():
        sys.exit(f"no crate for {program}: {path}")
    return path


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
        key = (licence["id"], licence["name"], licence["text"].strip())
        crates = {f'{u["crate"]["name"]} {u["crate"]["version"]}' for u in licence["used_by"]}
        found.setdefault(key, set()).update(crates)
    return found


def notices(package, programs):
    merged = {}
    for program in programs:
        for key, crates in licences(program).items():
            merged.setdefault(key, set()).update(crates)
    ours = {c for crates in merged.values() for c in crates if c.split()[0].startswith("bibli")}
    out = [
        f"Third-party software in {package}",
        "",
        *textwrap.wrap(
            f"The programs of this package ({', '.join(programs)}) are built from "
            "bibli's own code, under the package's licence, and the crates below, "
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
    apart = set(sys.argv[1:]) or set(lines("optional"))
    for package, programs in packages(apart).items():
        path = OUT / package / "THIRD-PARTY-NOTICES"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(notices(package, programs))
        print(f"{path.relative_to(ROOT)}: {len(programs)} programs")


if __name__ == "__main__":
    main()
