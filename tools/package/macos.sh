#!/bin/sh
# Package bibli for macOS (Phase 5, step K): `bibli.app` with its helpers
# beside the program, where it finds them, and a disk image of it. Run on
# a Mac, with Xcode's command line tools; written on Linux, never run yet
# (ADR-035: 5b runs it). Unsigned: signing and notarization need an Apple
# developer identity, a decision of its own.
#
#     tools/package/macos.sh [version]      # into target/package/
set -eu
version="${1:-0.0.1}"
root="$(cd "$(dirname "$0")/../.." && pwd)"
out="$root/target/package"
app="$out/bibli.app"
# The helpers a bundle carries (ADR-041): Mermaid and MuPDF are left out,
# as Linux's packages only suggest them.
helpers="bibli-pdf-worker bibli-model-worker bibli-module-worker"

cd "$root"
# The helpers that link native libraries (Poppler, MuPDF, GStreamer,
# libarchive) need those libraries in the bundle too: not done yet.
# shellcheck disable=SC2046 # one `-p name` per helper
cargo build --profile dist -p bibli $(for h in $helpers; do printf -- '-p %s ' "$h"; done)

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/dist/bibli "$app/Contents/MacOS/"
for h in $helpers; do cp "target/dist/$h" "$app/Contents/MacOS/"; done
# PDFium, which the PDF helper finds in the bundle's Frameworks (ADR-041):
# the release tools/package/pdfium.lock pins, checked against its sum.
case "$(uname -m)" in
arm64) platform=mac-arm64 ;;
*) platform=mac-x64 ;;
esac
tools/package/pdfium.sh "$platform" "$out/pdfium"
mkdir -p "$app/Contents/Frameworks" "$app/Contents/Resources/licenses"
cp "$out/pdfium/libpdfium.dylib" "$app/Contents/Frameworks/"
cp -R "$out/pdfium/licenses/pdfium" "$app/Contents/Resources/licenses/"
cp LICENSE "$app/Contents/Resources/licenses/"
# The type database, the fallback of the system's types (ADR-037), where
# bibli looks for it in a bundle: compiled from vendor/shared-mime-info
# (with Homebrew's update-mime-database: brew install shared-mime-info).
mkdir -p "$app/Contents/Resources/share"
tools/package/mime-database.sh "$app/Contents/Resources/share/mime"
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>bibli</string>
    <key>CFBundleDisplayName</key><string>bibli</string>
    <key>CFBundleIdentifier</key><string>io.github.venator.bibli</string>
    <key>CFBundleExecutable</key><string>bibli</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$version</string>
    <key>CFBundleVersion</key><string>$version</string>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

rm -f "$out/bibli-$version.dmg"
hdiutil create -volname bibli -srcfolder "$app" -ov -format UDZO "$out/bibli-$version.dmg"
echo "$out/bibli-$version.dmg"
