#!/usr/bin/env python3
"""Run the vendored GPUI list and line wrapper tests without adding GPUI dev
dependencies to skwir.

Covers our patches (item height hints, truncating the last clamped line,
keyboard clicks needing their key-down, the wheel scrolling along its axis)
alongside the upstream tests of those modules. Needs no GPU.
"""
from pathlib import Path
import shutil
import subprocess
import tempfile


root = Path(__file__).resolve().parents[2]
with tempfile.TemporaryDirectory(prefix="skwir-list-tests-") as temporary:
    source = Path(temporary) / "gpui"
    shutil.copytree(root / "vendor/gpui", source)
    manifest = source / "Cargo.toml"
    with manifest.open("a") as out:
        out.write("\n[workspace]\n")
    shutil.copyfile(root / "Cargo.lock", source / "Cargo.lock")
    subprocess.run([
        "cargo", "test", "--manifest-path", str(manifest),
        "--target-dir", str(root / "target"), "--no-default-features",
        "--features", "wayland", "--lib", "--",
        "elements::list", "elements::div::keyboard_click_tests",
        "elements::div::scroll_axis_tests",
        "text_system::line_wrapper", "text_system::line_layout",
    ], check=True)
