#!/bin/sh
# skwir for macOS, made on Linux (ADR-075): its programs built for Apple
# silicon against Apple's SDK, which `just macos-sdk` downloads, compiled
# by clang and linked by LLVM's ld64.lld through xmac's wrappers; then
# `skwir.app`, signed ad hoc, zipped and in a disk image. Not notarized:
# macOS asks before it opens. tools/package/macos-check.sh looks both over;
# nothing here can run them.
#
# What the bundle holds: tools/package/macos/programs in Contents/MacOS
# (skwir and the helpers that carry no library of their own), PDFium in
# Contents/Frameworks (ADR-041), and in Contents/Resources the icon, the
# type database (ADR-037) in share/mime, and in licenses skwir's licence,
# its crates' notices (tools/package/notices/macos) and PDFium's.
#
#     tools/package/macos.sh [version]      # into target/package/macos/
set -eu
if [ -z "${XMAC_CLANG:-}" ] || ! command -v rcodesign >/dev/null; then
    exec nix develop "$(cd "$(dirname "$0")/../.." && pwd)#macos" --command "$0" "$@"
fi
root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"
version="${1:-$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)}"
target=aarch64-apple-darwin
programs="$(grep -v '^#' tools/package/macos/programs)"

# The SDK and xmac's wrappers, as `just macos-sdk` left them.
xmac="${XDG_DATA_HOME:-$HOME/.local/share}/skwir-build/macos-sdk"
if [ ! -f "$xmac/env.sh" ]; then
    echo "No macOS SDK in $xmac: run \`just macos-sdk\` (Apple's licence is yours to accept)" >&2
    exit 1
fi
# shellcheck disable=SC2016 # env.sh's own words, `$_xmac_root` in them
SDKROOT="$xmac/$(sed -n 's/^export SDKROOT="\$_xmac_root\/\(.*\)"$/\1/p' "$xmac/env.sh")"
export SDKROOT MACOSX_DEPLOYMENT_TARGET=11.0
export CARGO_TARGET_DIR="$root/target/macos" CARGO_INCREMENTAL=0
# What a program says of where it was built (a panic's location, C's
# __FILE__) names no folder of this machine: the crates' sources are under
# /cargo, Rust's own under /rust, the tree under /skwir.
cargo_home="${CARGO_HOME:-$HOME/.cargo}"
sysroot="$(rustc --print sysroot)"
CARGO_ENCODED_RUSTFLAGS="$(printf -- '--remap-path-prefix=%s=/cargo\037--remap-path-prefix=%s=/rust\037--remap-path-prefix=%s=/skwir' \
    "$cargo_home" "$sysroot" "$root")"
export CARGO_ENCODED_RUSTFLAGS
export CFLAGS_aarch64_apple_darwin="-ffile-prefix-map=$cargo_home=/cargo -ffile-prefix-map=$root=/skwir"
export CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER="$xmac/bin/arm64-apple-darwin-cc"
export CC_aarch64_apple_darwin="$xmac/bin/arm64-apple-darwin-cc"
export CXX_aarch64_apple_darwin="$xmac/bin/arm64-apple-darwin-c++"
AR_aarch64_apple_darwin="$(command -v llvm-ar)"
export AR_aarch64_apple_darwin
# The dev shell's pkg-config answers with Linux's libraries: for macOS, a
# library is the SDK's (SQLite's), found by the linker.
export PKG_CONFIG_aarch64_apple_darwin=false
# shellcheck disable=SC2046,SC2086 # one `-p name` per program
cargo build --profile dist --target "$target" $(printf -- '-p %s ' $programs)

out="$root/target/package/macos"
name="skwir-$version-macos-arm64"
app="$out/skwir.app"
rm -rf "${app:?}" "$out/$name.zip" "$out/$name.dmg" "$out/work"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Frameworks" "$app/Contents/Resources/licenses" "$out/work"
for program in $programs; do
    cp "$CARGO_TARGET_DIR/$target/dist/$program" "$app/Contents/MacOS/"
done
tools/package/pdfium.sh mac-arm64 "$out/work/pdfium"
cp "$out/work/pdfium/libpdfium.dylib" "$app/Contents/Frameworks/"
cp -R "$out/work/pdfium/licenses/pdfium" "$app/Contents/Resources/licenses/"
cp LICENSE "$app/Contents/Resources/licenses/"
cp tools/package/notices/macos/skwir/THIRD-PARTY-NOTICES "$app/Contents/Resources/licenses/"
tools/package/mime-database.sh "$app/Contents/Resources/share/mime" >/dev/null

# The icon: the UI's own drawing (crates/ui/assets/org.skwir.skwir.svg) at
# every side an .icns holds, each a PNG under its type.
svg=crates/ui/assets/org.skwir.skwir.svg
for side in 16 32 64 128 256 512 1024; do
    rsvg-convert -w "$side" -h "$side" -o "$out/work/$side.png" "$svg"
done
python3 - "$out/work" "$app/Contents/Resources/skwir.icns" <<'EOF'
import pathlib, struct, sys
work, out = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2])
# Each type and the side of its picture: 16, 32 and 64 points, then the
# larger ones and each at twice its side (Retina).
types = [("icp4", 16), ("icp5", 32), ("icp6", 64), ("ic07", 128), ("ic08", 256),
         ("ic09", 512), ("ic10", 1024), ("ic11", 32), ("ic12", 64), ("ic13", 256),
         ("ic14", 512)]
body = b"".join(kind.encode() + struct.pack(">I", 8 + len(png)) + png
                for kind, png in ((k, (work / f"{s}.png").read_bytes()) for k, s in types))
out.write_bytes(b"icns" + struct.pack(">I", 8 + len(body)) + body)
EOF

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>skwir</string>
    <key>CFBundleDisplayName</key><string>skwir</string>
    <key>CFBundleIdentifier</key><string>org.skwir.skwir</string>
    <key>CFBundleExecutable</key><string>skwir</string>
    <key>CFBundleIconFile</key><string>skwir</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$version</string>
    <key>CFBundleVersion</key><string>$version</string>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

# Signed ad hoc, as Apple silicon asks of anything it runs: each program,
# PDFium, and the bundle's resources sealed.
rcodesign sign "$app" >/dev/null

# The zip: the bundle, its entries sorted, programs executable, dated by
# the commit.
python3 - "$out" "$name" "$(git log -1 --format=%ct)" <<'EOF'
import pathlib, sys, time, zipfile
out, name, stamp = pathlib.Path(sys.argv[1]), sys.argv[2], int(sys.argv[3])
when = time.gmtime(max(stamp, 315532800))[:6]
with zipfile.ZipFile(out / f"{name}.zip", "w", zipfile.ZIP_DEFLATED, compresslevel=9) as zipped:
    for path in sorted((out / "skwir.app").rglob("*")):
        if path.is_file():
            entry = zipfile.ZipInfo(str(path.relative_to(out)), when)
            entry.compress_type = zipfile.ZIP_DEFLATED
            entry.create_system = 3
            entry.external_attr = (0o100755 if path.stat().st_mode & 0o111 else 0o100644) << 16
            zipped.writestr(entry, path.read_bytes())
EOF

# The disk image: skwir.app beside a link to Applications, to drag it to,
# on an HFS+ volume made here (hfsprogs' mkfs.hfsplus), filled by
# libdmg-hfsplus's `hfsplus` (modes and links kept) and compressed by its
# `dmg`, as Firefox's are made on Linux. The volume has a fifth to spare.
mkdir -p "$out/work/volume"
cp -a "$app" "$out/work/volume/"
ln -s /Applications "$out/work/volume/Applications"
size="$(du -sm "$out/work/volume" | cut -f1)"
truncate -s "$((size + size / 5 + 16))M" "$out/work/skwir.hfs"
mkfs.hfsplus -v skwir "$out/work/skwir.hfs" >/dev/null
hfsplus "$out/work/skwir.hfs" addall "$out/work/volume/" --symlinks=clone_link >/dev/null
dmg build "$out/work/skwir.hfs" "$out/$name.dmg" >/dev/null
rm -rf "${out:?}/work"

for artifact in "$name.zip" "$name.dmg"; do
    (cd "$out" && sha256sum "$artifact" > "$artifact.sha256")
    echo "made: $out/$artifact"
done
