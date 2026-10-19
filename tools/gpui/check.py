#!/usr/bin/env python3
"""Run the vendored gpui crates' tests that cover Skwir's patches, alongside
upstream's tests of the same modules, on a copy of vendor/zed (a workspace of
its own), so gpui's dev dependencies stay out of Skwir's.

    check.py [crate] [-- cargo test arguments]

The text, list, scrolling and key tests need no GPU; gpui_wgpu's atlas and
renderer tests ask wgpu for any adapter (lavapipe will do). gpui's library
tests embed two of Zed's fonts, which the vendored crates leave out: they are
fetched once from Zed's repository at the vendored commit, into
~/.cache/skwir-tests.
"""
from pathlib import Path
import os
import shutil
import subprocess
import sys
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
VENDOR = ROOT / "vendor/zed"
COPY = Path.home() / ".cache/skwir-tests/gpui-check/zed"
FONTS = {
    "ibm-plex-sans": ["IBMPlexSans-Regular.ttf", "IBMPlexSans-Italic.ttf",
                      "IBMPlexSans-SemiBold.ttf", "IBMPlexSans-SemiBoldItalic.ttf"],
    "lilex": ["Lilex-Regular.ttf", "Lilex-Bold.ttf", "Lilex-Italic.ttf",
              "Lilex-BoldItalic.ttf"],
}
PATCHED = {"async-process", "async-task", "calloop", "khronos-egl"}
RUNS = [
    ("gpui", ["--no-default-features", "--features", "wayland,x11"], [
        "elements::list", "elements::div::scroll_axis_tests",
        "text_system::line_wrapper", "text_system::line_layout", "style::tests",
        # Upstream's, and what an action sees of a key held down (0032).
        "key_dispatch",
        # The accessibility tree (0030) and what an element writes into it
        # (0031).
        "window::a11y", "elements::div::tests::test_write_a11y_info",
        # A mesh and its normals (0033).
        "mesh::",
    ]),
    # All of them: the atlas (an upload to a removed texture), the context
    # and the renderer.
    ("gpui_wgpu", [], []),
    ("gpui_linux", [], ["clipboard"]),
]


def fonts(commit):
    cache = Path.home() / ".cache/skwir-tests/gpui-check/fonts" / commit
    for folder, names in FONTS.items():
        for name in names:
            path = cache / folder / name
            if path.is_file():
                continue
            path.parent.mkdir(parents=True, exist_ok=True)
            url = ("https://raw.githubusercontent.com/zed-industries/zed/"
                   f"{commit}/assets/fonts/{folder}/{name}")
            print(f"fetching {url}", flush=True)
            with urllib.request.urlopen(url, timeout=60) as response:
                data = response.read()
            path.with_suffix(".part").write_bytes(data)
            path.with_suffix(".part").rename(path)
    return cache


def main():
    args = sys.argv[1:]
    extra = args[args.index("--") + 1:] if "--" in args else []
    only = {a for a in (args[:args.index("--")] if "--" in args else args)}
    commit = (VENDOR / "UPSTREAM-COMMIT").read_text().split()[0]
    # One place, copied anew each run: the build reuses what did not change.
    shutil.rmtree(COPY, ignore_errors=True)
    shutil.copytree(VENDOR, COPY)
    shutil.copyfile(ROOT / "Cargo.lock", COPY / "Cargo.lock")
    # Skwir's root manifest repeats Zed's patches of the crates gpui reaches:
    # the tests build with them too.
    patches = [
        line for line in (ROOT / "Cargo.toml").read_text().splitlines()
        if line.split("=")[0].strip() in PATCHED and "git =" in line
    ]
    with (COPY / "Cargo.toml").open("a") as out:
        out.write("\n[patch.crates-io]\n" + "\n".join(patches) + "\n")
    shutil.copytree(fonts(commit), COPY / "assets/fonts")
    failed = []
    for crate, features, filters in RUNS:
        if only and crate not in only:
            continue
        command = [
            "cargo", "test", "--manifest-path", str(COPY / "Cargo.toml"), "-p", crate,
            "--target-dir", str(ROOT / "target/gpui-check"), *features, "--lib", *extra,
            "--", *filters,
        ]
        print("+", " ".join(command), flush=True)
        # A C object of a test dependency (alloca, by criterion) is built with
        # LTO, which mold cannot link.
        env = {**os.environ, "CFLAGS": "-fno-lto"}
        if subprocess.run(command, env=env).returncode != 0:
            failed.append(crate)
    sys.exit(f"failed: {', '.join(failed)}" if failed else 0)


if __name__ == "__main__":
    main()
