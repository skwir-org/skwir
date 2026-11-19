#!/usr/bin/env python3
"""The crates Skwir vendors by path (`[patch.crates-io]` in Cargo.toml),
held to the advisories as the releases they were taken from.

    tools/deny/vendored.py [cargo-deny options]    # `just deny` runs it

cargo-deny matches an advisory only to a crate from a registry: a crate
taken by path has no source, and `cargo deny check` passes over it in
silence (an advisory made up for jpeg-decoder 0.3.2 went unreported,
one for png reported, 2026-11-19). So a manifest of its own, in a
temporary folder, depends on each vendored crate's published release (the
name and version its own Cargo.toml says), and `cargo deny check
advisories` runs on it with deny.toml: what it names directly counts as
the workspace's (`unmaintained = "workspace"`), as Skwir's crates name
them.
"""

import pathlib
import subprocess
import sys
import tempfile
import tomllib

ROOT = pathlib.Path(__file__).resolve().parents[2]


def vendored(root=ROOT):
    """(name, version) of every crate `[patch.crates-io]` takes by path."""
    manifest = tomllib.loads((root / "Cargo.toml").read_text())
    crates = []
    for entry in manifest.get("patch", {}).get("crates-io", {}).values():
        if "path" not in entry:
            continue
        own = tomllib.loads((root / entry["path"] / "Cargo.toml").read_text())
        # Build metadata (two-face's `+bat-0.26.1`) names no other release.
        crates.append((own["package"]["name"], own["package"]["version"].split("+")[0]))
    return sorted(crates)


def manifest(crates):
    """A manifest depending on exactly these releases, and nothing else."""
    lines = [
        "[package]",
        'name = "skwir-vendored-advisories"',
        'version = "0.0.0"',
        'edition = "2021"',
        "publish = false",
        "",
        "[workspace]",
        "",
        "[dependencies]",
    ]
    lines += [
        f'{name} = {{ version = "={version}", default-features = false }}'
        for name, version in crates
    ]
    return "\n".join(lines) + "\n"


def main(options):
    crates = vendored()
    if not crates:
        return 0
    print("vendored:", ", ".join(f"{n} {v}" for n, v in crates), flush=True)
    if "--config" not in options:
        options = [*options, "--config", str(ROOT / "deny.toml")]
    with tempfile.TemporaryDirectory(prefix="skwir-vendored-") as folder:
        folder = pathlib.Path(folder)
        (folder / "Cargo.toml").write_text(manifest(crates))
        (folder / "src").mkdir()
        (folder / "src" / "lib.rs").write_text("")
        command = ["cargo", "deny", "--manifest-path", str(folder / "Cargo.toml")]
        return subprocess.call([*command, *options, "check", "advisories"])


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
