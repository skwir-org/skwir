#!/bin/sh
# The type database the macOS and Windows packages carry beside skwir:
# the fallback of the system's own types (ADR-037). Compiled from the
# source vendored in vendor/shared-mime-info by shared-mime-info's own
# `update-mime-database` (in the dev shell; on a Mac, Homebrew's
# `shared-mime-info`), and only the files skwir reads, with the licence
# (GPL-2.0-or-later: data of its own beside skwir, never built into it)
# and a note of where its source is.
#
#     tools/package/mime-database.sh <out>
#
# Windows has no `update-mime-database`: its package is made on Linux,
# where `tools/package/windows.sh` runs this (ADR-074).
set -eu
if [ $# -ne 1 ]; then
    echo "usage: $0 <out>" >&2
    exit 2
fi
root="$(cd "$(dirname "$0")/../.." && pwd)"
source="$root/vendor/shared-mime-info"
out="$1"
if ! command -v update-mime-database >/dev/null; then
    echo "update-mime-database is needed (shared-mime-info: the dev shell has it; on a Mac, brew install shared-mime-info)" >&2
    exit 1
fi
version="$(cat "$source/VERSION")"
build="$out.build"
trap 'rm -rf "${build:?}"' EXIT
rm -rf "${build:?}" "${out:?}"
mkdir -p "$build/mime/packages" "$out"
cp "$source/freedesktop.org.xml.in" "$build/mime/packages/freedesktop.org.xml"
# Named as a data folder, so it does not say it is not one.
XDG_DATA_DIRS="$build" update-mime-database "$build/mime"
# What skwir reads: the xdg-mime crate, and `platform::mime` itself.
for f in aliases generic-icons globs2 icons magic subclasses; do
    cp "$build/mime/$f" "$out/$f"
done
cp "$source/COPYING" "$out/COPYING"
cat > "$out/README" <<README
shared-mime-info's type database, release $version, compiled
(https://gitlab.freedesktop.org/xdg/shared-mime-info). Its source is
in skwir's source, vendor/shared-mime-info, and upstream at that release.

Licensed under the GNU General Public License, version 2 or later
(COPYING), apart from skwir, which reads it as data.
README
echo "$out"
