#!/usr/bin/env python3
"""Run the vendored GPUI list tests without adding GPUI dev dependencies to bibli.

Covers our list patch (item height hints) alongside the upstream list tests.
Needs no GPU.
"""
from pathlib import Path
import shutil
import subprocess
import tempfile


root = Path(__file__).resolve().parents[2]
with tempfile.TemporaryDirectory(prefix="bibli-list-tests-") as temporary:
    source = Path(temporary) / "gpui"
    shutil.copytree(root / "vendor/gpui", source)
    manifest = source / "Cargo.toml"
    with manifest.open("a") as out:
        out.write("\n[workspace]\n")
    shutil.copyfile(root / "Cargo.lock", source / "Cargo.lock")
    subprocess.run([
        "cargo", "test", "--manifest-path", str(manifest),
        "--target-dir", str(root / "target"), "--no-default-features",
        "--features", "wayland", "--lib", "elements::list",
    ], check=True)
