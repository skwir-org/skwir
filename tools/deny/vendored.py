#!/usr/bin/env python3
"""The crates Skwir takes in place of a release (`[patch.crates-io]` in
Cargo.toml: vendored by path, or a fork from git), held to the advisories
as the releases they stand for.

    tools/deny/vendored.py [cargo-deny options]    # `just deny` runs it

cargo-deny matches an advisory only to a crate from a registry: a crate
taken by path has no source, one from git a source of its own, and `cargo
deny check` passes over both in silence (advisories made up for
jpeg-decoder 0.3.2, by path, went unreported, one for png reported,
2026-11-19; for async-task 4.7.1 and oo7 0.6.0, patched from git, and
wgpu 30.0.0, gpui's from git, unreported, 2026-11-20). So a manifest of
its own, in a temporary folder, depends on each one's published release
(the name and version its own Cargo.toml says, or for a fork from git
what Cargo.lock locks), and `cargo deny check advisories` runs on it
with deny.toml: what it names directly counts as the workspace's
(`unmaintained = "workspace"`), as Skwir's crates name them. An advisory
for a release applies to a fork of it unless the fork fixed it, and is
then ignored in deny.toml, its reason said. The crates gpui itself takes
from git, at commits whose version crates.io may not hold (wgpu's, Zed's
forks of font-kit, xim and scap), are not held here: deny.toml says so.
Nor are copies that stand for no release, taken by path in
`[workspace.dependencies]` rather than patched: skwird's client and
protocol (vendor/skwird-*, published nowhere, ADR-050), whose own
dependencies come from crates.io and are checked as Skwir's.
"""

import pathlib
import subprocess
import sys
import tempfile
import tomllib

ROOT = pathlib.Path(__file__).resolve().parents[2]


def vendored(root=ROOT):
    """(name, version) of every crate `[patch.crates-io]` takes in place of
    a release: by path, the version its own Cargo.toml says; from git, the
    version Cargo.lock locks from that repository (none for a patch no
    build uses)."""
    manifest = tomllib.loads((root / "Cargo.toml").read_text())
    locked = None
    crates = []
    for key, entry in manifest.get("patch", {}).get("crates-io", {}).items():
        if "path" in entry:
            own = tomllib.loads((root / entry["path"] / "Cargo.toml").read_text())
            name, version = own["package"]["name"], own["package"]["version"]
            # Build metadata (two-face's `+bat-0.26.1`) names no other release.
            crates.append((name, version.split("+")[0]))
        elif "git" in entry:
            if locked is None:
                locked = tomllib.loads((root / "Cargo.lock").read_text())["package"]
            name = entry.get("package", key)
            source = "git+" + entry["git"]
            crates += sorted(
                {
                    (name, package["version"].split("+")[0])
                    for package in locked
                    if package["name"] == name
                    and package.get("source", "").split("?")[0].split("#")[0] == source
                }
            )
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
