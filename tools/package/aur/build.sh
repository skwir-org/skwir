#!/bin/sh
# The AUR package built from this tree as it would be from a release
# (ADR-040): the tree's files as the release's source tarball
# (`bibli-<version>/…`, as GitHub lays it out), then makepkg with the
# PKGBUILD unchanged, in target/aur. On Arch, not as root; arguments go
# to makepkg (`--syncdeps` installs what it needs through sudo).
#
#     tools/package/aur/build.sh
set -eu
root="$(cd "$(dirname "$0")/../../.." && pwd)"
version="$(sed -n 's/^pkgver=//p' "$root/tools/package/aur/PKGBUILD")"
# The PDFium the PKGBUILD names is the one pdfium.lock pins.
lock="$root/tools/package/pdfium.lock"
if [ "chromium/$(sed -n 's/^_pdfium=//p' "$root/tools/package/aur/PKGBUILD")" != "$(sed -n 's/^release //p' "$lock")" ]; then
    echo "the PKGBUILD's PDFium is not the release pdfium.lock pins" >&2
    exit 1
fi
for asset in pdfium-linux-x64.tgz pdfium-linux-arm64.tgz; do
    sum="$(awk -v asset="$asset" '$1 == asset { print $2 }' "$lock")"
    if ! grep -q "'$sum'" "$root/tools/package/aur/PKGBUILD"; then
        echo "the PKGBUILD's sum of $asset is not pdfium.lock's" >&2
        exit 1
    fi
done
work="$root/target/aur"
rm -rf "${work:?}"
mkdir -p "$work"
cp "$root/tools/package/aur/PKGBUILD" "$work/"
# The tracked files and those not ignored, as a release's tag holds them;
# where there is no repository, everything but the build folder.
if git -C "$root" rev-parse --git-dir >/dev/null 2>&1; then
    (cd "$root" && git ls-files -co --exclude-standard) > "$work/files"
else
    (cd "$root" && find . -path ./target -prune -o -type f -print | sed 's,^\./,,') > "$work/files"
fi
tar -C "$root" -T "$work/files" --transform "s,^,bibli-$version/," -czf "$work/bibli-$version.tar.gz"
cd "$work"
makepkg --noconfirm "$@"
