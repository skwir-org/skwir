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
