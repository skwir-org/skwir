#!/bin/sh
# skwir for macOS, made on Linux (ADR-075): its programs built for Apple
# silicon against Apple's SDK, which `just macos-sdk` downloads, compiled
# by clang and linked by LLVM's ld64.lld through xmac's wrappers; then
# `skwir.app`, signed ad hoc, zipped and in a disk image. Not notarized:
# macOS asks before it opens. tools/package/macos-check.sh looks both over;
# nothing here can run them.
#
# What the bundle holds: tools/package/macos/programs in Contents/MacOS
# (skwir and the helpers that carry no library of their own, the archive
# helper, with libarchive linked in, and the media helper), skwird beside
# them, its own release's build (ADR-050, amended 2026-11-26), PDFium in
# Contents/Frameworks (ADR-041) and GStreamer, cut down to what the media
# helper plays with, a framework in Contents/Frameworks (ADR-076), and in
# Contents/Resources the icon, an .lproj for each language skwir speaks
# (ADR-139), the type database (ADR-037) in share/mime,
# and in licenses skwir's licence, its crates' notices
# (tools/package/notices/macos), PDFium's, skwird's and its crates',
# libarchive's and its compressors' (tools/package/libarchive.sh), and
# GStreamer's and its components' (tools/package/gstreamer.py).
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
export SDKROOT MACOSX_DEPLOYMENT_TARGET=12.0
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
# GStreamer's own build, cut down (tools/package/gstreamer.py), which the
# media helper is linked against. pkg-config is asked for macOS of its
# files alone, not the dev shell's, which would answer with Linux's
# libraries: a library it has none of is the SDK's, found by the linker.
gstreamer="$(tools/package/gstreamer.py "$target")"
export PKG_CONFIG_ALLOW_CROSS=1
export PKG_CONFIG_PATH_aarch64_apple_darwin="$gstreamer/link/lib/pkgconfig"
export PKG_CONFIG_LIBDIR_aarch64_apple_darwin="$gstreamer/link/lib/pkgconfig"
# libarchive, static, which the archive helper links (its build.rs) with
# the system's zlib, bzip2, iconv and Expat: built here once for each
# version.
SKWIR_LIBARCHIVE_STATIC="$(tools/package/libarchive.sh "$target")"
export SKWIR_LIBARCHIVE_STATIC
# SQLite built in: macOS 12 to 14 have one older than rusqlite needs, and
# the SDK's, newer, would link symbols they lack (ADR-022).
# shellcheck disable=SC2046,SC2086 # one `-p name` per program
cargo build --profile dist --target "$target" --features skwir/bundled-sqlite $(printf -- '-p %s ' $programs)

out="$root/target/package/macos"
name="skwir-$version-macos-arm64"
app="$out/skwir.app"
rm -rf "${app:?}" "$out/$name.zip" "$out/$name.dmg" "$out/work"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Frameworks" "$app/Contents/Resources/licenses" "$out/work"
for program in $programs; do
    cp "$CARGO_TARGET_DIR/$target/dist/$program" "$app/Contents/MacOS/"
done
# skwird, skwir's index of names (ADR-050), as its own release built it
# (tools/package/skwird.lock; tools/package/skwird.py checks the archive's
# sum before it unpacks it), beside skwir in Contents/MacOS, where skwir
# looks for it after PATH: signed ad hoc in its archive, signed again
# with the bundle below, as the other programs are. Its licences and its
# notices in Resources/licenses/skwird, as its archive holds them. A disk
# image has no uninstaller: before the app goes, its user takes back a
# LaunchAgent this copy registered (`skwird logon remove --if-mine`) and
# stops its engine (`skwird stop`), as docs/guide.md says.
python3 tools/package/skwird.py macos "$out/work/skwird"
cp "$out/work/skwird/skwird" "$app/Contents/MacOS/"
cp -R "$out/work/skwird/licenses/skwird" "$app/Contents/Resources/licenses/"
tools/package/pdfium.sh mac-arm64 "$out/work/pdfium"
cp "$out/work/pdfium/libpdfium.dylib" "$app/Contents/Frameworks/"
cp -R "$out/work/pdfium/licenses/pdfium" "$app/Contents/Resources/licenses/"
# Skwir's own fonts, read when they are drawn (ADR-118).
python3 tools/package/fonts.py "$out/work/fonts"
mkdir -p "$app/Contents/Resources/share/skwir"
cp -R "$out/work/fonts/fonts" "$app/Contents/Resources/share/skwir/"
# How the copy came (ADR-142).
cp tools/package/channel/macos "$app/Contents/Resources/share/skwir/channel"
cp -R "$out/work/fonts/licenses/fonts" "$app/Contents/Resources/licenses/"
cp -R "$SKWIR_LIBARCHIVE_STATIC/licenses/." "$app/Contents/Resources/licenses/"
cp -R "$gstreamer/runtime/GStreamer.framework" "$app/Contents/Frameworks/"
cp -R "$gstreamer/licenses/." "$app/Contents/Resources/licenses/"
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

# The languages skwir speaks (ADR-139, 13): English, and each it offers,
# reviewed or as a draft (tools/words/offered, kept by `just words`: a
# line's first word is its tag), declared so that macOS's own
# menus and its per-app language follow, each an .lproj saying what
# Info.plist says to a person (the brand alone, the same in every
# language). A language only shipped, not offered, is left out: Automatic
# never speaks it, and a Mac in it would mix macOS's menus in it with
# skwir's English. The development region is English.
languages="en $(sed '/^#/d; /^[[:space:]]*$/d' tools/words/offered | awk '{ print $1 }' | LC_ALL=C sort -u | tr '\n' ' ')"
for language in $languages; do
    mkdir -p "$app/Contents/Resources/$language.lproj"
    cat > "$app/Contents/Resources/$language.lproj/InfoPlist.strings" <<'STRINGS'
/* What Info.plist says to a person: skwir's name, the brand, the same in every language. */
CFBundleName = "skwir";
CFBundleDisplayName = "skwir";
STRINGS
done

# shellcheck disable=SC2086 # one <string> per language
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>skwir</string>
    <key>CFBundleDisplayName</key><string>skwir</string>
    <key>CFBundleIdentifier</key><string>org.skwir.skwir</string>
    <key>CFBundleDevelopmentRegion</key><string>en</string>
    <key>CFBundleLocalizations</key><array>$(printf '<string>%s</string>' $languages)</array>
    <key>CFBundleExecutable</key><string>skwir</string>
    <key>CFBundleIconFile</key><string>skwir</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$version</string>
    <key>CFBundleVersion</key><string>$version</string>
    <key>LSMinimumSystemVersion</key><string>12.0</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

# Signed ad hoc, as Apple silicon asks of anything it runs: each program,
# PDFium, and the bundle's resources sealed.
rcodesign sign "$app" >/dev/null

# The zip: the bundle, its entries sorted, programs executable, links
# kept as links (the GStreamer framework's), dated by the commit.
python3 - "$out" "$name" "$(git log -1 --format=%ct)" <<'EOF'
import pathlib, sys, time, zipfile
out, name, stamp = pathlib.Path(sys.argv[1]), sys.argv[2], int(sys.argv[3])
when = time.gmtime(max(stamp, 315532800))[:6]
with zipfile.ZipFile(out / f"{name}.zip", "w", zipfile.ZIP_DEFLATED, compresslevel=9) as zipped:
    for path in sorted((out / "skwir.app").rglob("*")):
        if path.is_symlink():
            entry = zipfile.ZipInfo(str(path.relative_to(out)), when)
            entry.create_system = 3
            entry.external_attr = 0o120755 << 16
            zipped.writestr(entry, str(path.readlink()))
        elif path.is_file():
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
