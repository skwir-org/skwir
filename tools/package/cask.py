#!/usr/bin/env python3
"""Skwir's Homebrew cask of a release (ADR-075, ADR-142): what skwir-org's tap
holds as `Casks/skwir.rb`, so `brew install --cask skwir-org/tap/skwir`
installs the Mac app and `brew upgrade` updates it. Made from the release's
bundle, which `release-build` made and `release-verify` checked: its version
and the zip's SHA-256 from its manifest, the oldest macOS from the Info.plist
in that zip (not the working tree's, which may have moved on since the tag),
the zip at the release's own address, its bytes as they are (Homebrew
quarantines them, as a browser does).

    tools/package/cask.py target/releases/v0.7.0 [--tap FOLDER]

writes `Casks/skwir.rb` in FOLDER, a checkout of skwir-org/homebrew-tap (by
default `../homebrew-tap`, beside this repository). Committing and pushing it
is the user's, once the release is published: the tap's workflow installs the
zip from the release's page (`docs/RELEASING.md`). Nothing here reaches the
network.

The cask never takes the quarantine off, in a postflight or otherwise: the app
is signed ad hoc and not notarized, and macOS asks before its first start
(Open Anyway, in Privacy & Security), as it asks of the zip downloaded by hand.
Its caveats say so.
"""

import argparse
import hashlib
import json
import pathlib
import plistlib
import re
import sys
import zipfile

ROOT = pathlib.Path(__file__).resolve().parents[2]
REPOSITORY = "https://github.com/skwir-org/skwir"
TAP = "skwir-org/tap"
# The app's own Info.plist in the release's zip, which has `skwir.app` at its root.
INFO_PLIST = "skwir.app/Contents/Info.plist"
# Homebrew's name of each macOS. A cask names the oldest it runs on by
# that name alone (`depends_on macos: :monterey`): its style refuses the
# string `">= :monterey"` since 2026-11-22 (its cop Homebrew/OSDependsOn),
# and the tap's workflow runs that style.
MACOS_NAMES = {"12": "monterey", "13": "ventura", "14": "sonoma", "15": "sequoia", "26": "tahoe"}


def summary():
    """What Skwir is, in a line, as the desktop's software centres say it."""
    metainfo = (ROOT / "tools/package/linux/org.skwir.skwir.metainfo.xml").read_text()
    return re.search(r"<summary>([^<]+)</summary>", metainfo).group(1)


def minimum_macos(zipped):
    """The oldest macOS the app in the release's zip `zipped` runs on, as the
    Info.plist the cask installs says it (`LSMinimumSystemVersion`, ADR-075),
    by Homebrew's name for it."""
    try:
        with zipfile.ZipFile(zipped) as archive:
            info = plistlib.loads(archive.read(INFO_PLIST))
    except (OSError, KeyError, zipfile.BadZipFile, plistlib.InvalidFileException) as error:
        sys.exit(f"{zipped.name}: no {INFO_PLIST} read: {error}")
    version = info.get("LSMinimumSystemVersion")
    if not isinstance(version, str) or not re.fullmatch(r"[0-9]+(\.[0-9]+)*", version):
        sys.exit(f"{zipped.name}: its Info.plist names no oldest macOS (LSMinimumSystemVersion)")
    major = version.split(".")[0]
    if major not in MACOS_NAMES:
        sys.exit(f"macOS {version}: no Homebrew name known for it (tools/package/cask.py)")
    return MACOS_NAMES[major]


def sha256_of(path):
    digest = hashlib.sha256()
    with path.open("rb") as file:
        while chunk := file.read(1 << 20):
            digest.update(chunk)
    return digest.hexdigest()


def cask(bundle, manifest):
    """The cask of the release `manifest` describes, its assets in `bundle`."""
    version = manifest["version"]
    zipped = f"skwir-{version}-macos-arm64.zip"
    if zipped not in manifest["assets"]:
        sys.exit(f"{manifest['tag']}: no {zipped} in the release (made without the macOS SDK?)")
    sha256 = manifest["assets"][zipped]["sha256"].lower()
    if not re.fullmatch(r"[0-9a-f]{64}", sha256):
        sys.exit(f"{zipped}: not a SHA-256: {sha256}")
    # The macOS it needs is read from the zip the cask names: that one's.
    if not (bundle / zipped).is_file():
        sys.exit(f"{bundle}: no {zipped} in the bundle")
    if sha256_of(bundle / zipped) != sha256:
        sys.exit(f"{zipped}: not the zip its manifest names (run `just release-verify`)")
    oldest = minimum_macos(bundle / zipped)
    return f"""\
# Skwir, a file manager for humans and agents (skwir.org): the Mac app of
# its release, the release's own zip. Written at each release by skwir's
# `just cask <tag>` (tools/package/cask.py), not by hand.
cask "skwir" do
  version "{version}"
  sha256 "{sha256}"

  url "{REPOSITORY}/releases/download/v#{{version}}/skwir-#{{version}}-macos-arm64.zip",
      verified: "github.com/skwir-org/skwir/"
  name "Skwir"
  desc "{summary()}"
  homepage "https://skwir.org/"

  livecheck do
    url :url
    strategy :github_latest
  end

  depends_on arch: :arm64
  depends_on macos: :{oldest}

  app "skwir.app"
  binary "#{{appdir}}/skwir.app/Contents/MacOS/skwir"
  binary "#{{appdir}}/skwir.app/Contents/MacOS/skwir", target: "skw"

  zap trash: [
    "~/Library/Application Support/skwir",
    "~/Library/Caches/skwir",
    "~/Library/Preferences/org.skwir.skwir.plist",
    "~/Library/Saved Application State/org.skwir.skwir.savedState",
  ]

  caveats <<~EOS
    skwir is signed ad hoc and not notarized by Apple, so macOS refuses
    its first start. Open it once, then choose Open Anyway beside skwir
    in System Settings > Privacy & Security.

    Homebrew loads this tap's casks once the tap is trusted:
      brew trust {TAP}
  EOS
end
"""


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("bundle", type=pathlib.Path)
    parser.add_argument("--tap", type=pathlib.Path, default=ROOT.parent / "homebrew-tap")
    args = parser.parse_args()
    bundle = args.bundle.resolve()
    if not (bundle / "manifest.json").is_file():
        sys.exit(f"{bundle}: no release bundle (run `just release-build <tag>` first)")
    tap = args.tap.resolve()
    if not (tap / ".git").exists():
        sys.exit(f"{tap}: not a checkout of skwir-org/homebrew-tap (give it with --tap)")
    text = cask(bundle, json.loads((bundle / "manifest.json").read_text()))
    out = tap / "Casks" / "skwir.rb"
    out.parent.mkdir(exist_ok=True)
    out.write_text(text)
    print(f"made: {out}")


if __name__ == "__main__":
    main()
