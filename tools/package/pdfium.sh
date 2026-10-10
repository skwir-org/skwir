#!/bin/sh
# PDFium as skwir's packages carry it (ADR-041): the release pinned in
# tools/package/pdfium.lock, from bblanchon/pdfium-binaries, checked
# against its sum and unpacked into <out>: the library as the system
# names it (libpdfium.so, libpdfium.dylib, pdfium.dll) and its notices
# (licenses/pdfium/, PDFium's own and those of what it carries).
#
#     tools/package/pdfium.sh <linux-x64|linux-arm64|mac-univ|win-x64|…> <out>
set -eu
if [ $# -ne 2 ]; then
    echo "usage: $0 <platform> <out>" >&2
    exit 2
fi
lock="$(cd "$(dirname "$0")" && pwd)/pdfium.lock"
asset="pdfium-$1.tgz"
out="$2"
release="$(sed -n 's/^release //p' "$lock")"
sum="$(awk -v asset="$asset" '$1 == asset { print $2 }' "$lock")"
if [ -z "$sum" ]; then
    echo "$asset: not in $lock" >&2
    exit 1
fi
work="$out/.pdfium"
rm -rf "${work:?}"
mkdir -p "$work/unpacked" "$out/licenses/pdfium"
tag="$(printf %s "$release" | sed 's,/,%2F,')"
curl -fsSL -o "$work/$asset" "https://github.com/bblanchon/pdfium-binaries/releases/download/$tag/$asset"
if command -v sha256sum >/dev/null; then
    got="$(sha256sum "$work/$asset" | cut -d' ' -f1)"
else
    got="$(shasum -a 256 "$work/$asset" | cut -d' ' -f1)"
fi
if [ "$got" != "$sum" ]; then
    echo "$asset: its sum is not the one pinned" >&2
    exit 1
fi
tar --no-same-owner -xzf "$work/$asset" -C "$work/unpacked"
case "$1" in
linux-*) cp "$work/unpacked/lib/libpdfium.so" "$out/" ;;
mac-*) cp "$work/unpacked/lib/libpdfium.dylib" "$out/" ;;
win-*) cp "$work/unpacked/bin/pdfium.dll" "$out/" ;;
esac
cp "$work/unpacked/LICENSE" "$out/licenses/pdfium/LICENSE"
cp "$work/unpacked/licenses/"* "$out/licenses/pdfium/"
printf '%s\n' "$release" > "$out/licenses/pdfium/RELEASE"
rm -rf "${work:?}"
