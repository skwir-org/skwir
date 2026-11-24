#!/usr/bin/env python3
"""Skwir's Homebrew cask of a release (ADR-075, ADR-142): what skwir-org's tap
holds as `Casks/skwir.rb`, so `brew install --cask skwir-org/tap/skwir`
installs the Mac app and `brew upgrade` updates it. Made from the release's
bundle, which `release-build` made and `release-verify` checked: its version
and the zip's SHA-256 from its manifest, the zip at the release's own address,
its bytes as they are (Homebrew quarantines them, as a browser does).

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
import json
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
REPOSITORY = "https://github.com/skwir-org/skwir"
TAP = "skwir-org/tap"


def summary():
    """What Skwir is, in a line, as the desktop's software centres say it."""
    metainfo = (ROOT / "tools/package/linux/org.skwir.skwir.metainfo.xml").read_text()
    return re.search(r"<summary>([^<]+)</summary>", metainfo).group(1)


def minimum_macos():
    """The oldest macOS the app runs on, as its Info.plist says it
    (`LSMinimumSystemVersion`, ADR-075), by Homebrew's name for it."""
    script = (ROOT / "tools/package/macos.sh").read_text()
    version = re.search(r"<key>LSMinimumSystemVersion</key><string>([0-9.]+)</string>", script).group(1)
    names = {"12": "monterey", "13": "ventura", "14": "sonoma", "15": "sequoia", "26": "tahoe"}
    major = version.split(".")[0]
    if major not in names:
        sys.exit(f"macOS {version}: no Homebrew name known for it (tools/package/cask.py)")
    return names[major]


def cask(manifest):
    """The cask of the release `manifest` describes."""
    version = manifest["version"]
    zipped = f"skwir-{version}-macos-arm64.zip"
    if zipped not in manifest["assets"]:
        sys.exit(f"{manifest['tag']}: no {zipped} in the release (made without the macOS SDK?)")
    sha256 = manifest["assets"][zipped]["sha256"].lower()
    if not re.fullmatch(r"[0-9a-f]{64}", sha256):
        sys.exit(f"{zipped}: not a SHA-256: {sha256}")
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
  depends_on macos: ">= :{minimum_macos()}"

  app "skwir.app"
  binary "#{{appdir}}/skwir.app/Contents/MacOS/skwir"

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
    text = cask(json.loads((bundle / "manifest.json").read_text()))
    out = tap / "Casks" / "skwir.rb"
    out.parent.mkdir(exist_ok=True)
    out.write_text(text)
    print(f"made: {out}")


if __name__ == "__main__":
    main()
