#!/usr/bin/env python3
"""Build the Mac executable natively with the release's unchanged runtime assets.

This is a narrow integration-test bundle, not the cross-platform release pipeline.
The native helpers and their libraries come from a checksum-pinned upstream bundle;
only Skwir's executable and its shared bundle metadata are replaced. No file or
helper protocol changes are made by the macOS reveal fix.
"""

import argparse
import hashlib
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tomllib
import urllib.request


ROOT = Path(__file__).resolve().parents[2]
BASE_VERSION = "0.7.2"
BASE_SHA256 = "717588776b03f9171063e22c179c0eae43dcfb3e0f7650fedb873a8b3cb658f7"
BASE_URL = (
    "https://github.com/skwir-org/skwir/releases/download/"
    f"v{BASE_VERSION}/skwir-{BASE_VERSION}-macos-arm64.zip"
)


def run(*args, **kwargs):
    subprocess.run(args, cwd=ROOT, check=True, **kwargs)


def set_version(version):
    """Keep downstream prerelease numbering out of an upstream patch."""
    if not re.fullmatch(r"\d+\.\d+\.\d+(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?", version):
        raise ValueError(f"not a release version: {version!r}")
    manifest = ROOT / "Cargo.toml"
    original = manifest.read_text()
    old = tomllib.loads(original)["workspace"]["package"]["version"]
    if old == version:
        return
    changed, count = re.subn(
        rf'(?m)^version = "{re.escape(old)}"$',
        f'version = "{version}"',
        original,
        count=1,
    )
    if count != 1:
        raise ValueError("workspace version not found exactly once")
    manifest.write_text(changed)
    lock = ROOT / "Cargo.lock"
    blocks = lock.read_text().split("[[package]]")
    for index, block in enumerate(blocks[1:], 1):
        entry = tomllib.loads(block)
        if "source" not in entry and entry.get("version") == old:
            blocks[index] = block.replace(
                f'version = "{old}"', f'version = "{version}"', 1
            )
    lock.write_text("[[package]]".join(blocks))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", default="")
    args = parser.parse_args()
    if sys.platform != "darwin" or os.uname().machine != "arm64":
        parser.error("the native bundle must be built on an Apple silicon Mac")

    version = args.version or tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
    set_version(version)
    run("cargo", "build", "--locked", "--profile", "dist", "-p", "skwir", "--features", "bundled-sqlite")

    out = ROOT / "target/package/macos-native"
    out.mkdir(parents=True, exist_ok=True)
    archive = out / f"upstream-{BASE_VERSION}.zip"
    if not archive.exists() or hashlib.sha256(archive.read_bytes()).hexdigest() != BASE_SHA256:
        urllib.request.urlretrieve(BASE_URL, archive)
    if hashlib.sha256(archive.read_bytes()).hexdigest() != BASE_SHA256:
        raise ValueError("the upstream runtime bundle does not match its pinned checksum")
    app = out / "skwir.app"
    if app.exists():
        shutil.rmtree(app)
    run("ditto", "-x", "-k", str(archive), str(out))
    archive.unlink()
    target = Path(os.environ.get("CARGO_TARGET_DIR", "target"))
    if not target.is_absolute():
        target = ROOT / target
    shutil.copy2(target / "dist/skwir", app / "Contents/MacOS/skwir")
    run(sys.executable, "tools/package/macos-plist.py", str(app), version)
    run("codesign", "--force", "--deep", "--sign", "-", str(app))
    run("codesign", "--verify", "--deep", "--strict", str(app))
    actual = subprocess.check_output([str(app / "Contents/MacOS/skwir"), "--version"], text=True)
    if not actual.startswith(f"skwir {version}"):
        raise ValueError(f"the compiled version disagrees with the bundle: {actual!r}")
    print(actual.strip(), flush=True)
    packaged = out / f"skwir-{version}-macos-arm64.zip"
    if packaged.exists():
        packaged.unlink()
    run("ditto", "-c", "-k", "--sequesterRsrc", "--keepParent", str(app), str(packaged))
    digest = hashlib.sha256(packaged.read_bytes()).hexdigest()
    packaged.with_suffix(".zip.sha256").write_text(f"{digest}  {packaged.name}\n")
    print(f"Verified native bundle: {packaged}\nSHA-256: {digest}", flush=True)


if __name__ == "__main__":
    main()
