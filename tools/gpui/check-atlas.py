#!/usr/bin/env python3
"""Run Blade atlas GPU regressions without adding GPUI dev dependencies to skwir.

Run inside the Linux development shell. Requires a working GPU context.
"""
from pathlib import Path
import shutil
import subprocess
import tempfile


root = Path(__file__).resolve().parents[2]
with tempfile.TemporaryDirectory(prefix="skwir-atlas-tests-") as temporary:
    source = Path(temporary) / "gpui"
    shutil.copytree(root / "vendor/gpui", source)
    manifest = source / "Cargo.toml"
    with manifest.open("a") as out:
        out.write("\n[workspace]\n")
    shutil.copyfile(root / "Cargo.lock", source / "Cargo.lock")
    subprocess.run([
        "cargo", "test", "--manifest-path", str(manifest),
        "--target-dir", str(root / "target"), "--no-default-features",
        "--features", "wayland", "--lib",
        "platform::blade::blade_atlas::tests", "--", "--ignored",
    ], check=True)
