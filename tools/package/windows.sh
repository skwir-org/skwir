#!/bin/sh
# skwir for Windows, made on Linux (ADR-074): its programs built with
# cargo-zigbuild (MinGW-w64, as `just check-windows` and Wine's tests build
# them), then a folder of them, zipped to be used where it is unpacked,
# and an installer of the same folder, by msitools' wixl. Unsigned: Windows
# warns before either runs. tools/package/windows-check.sh tries both
# under Wine.
#
# What the folder holds: tools/package/windows/programs (skwir and the
# helpers that carry no library of their own), PDFium beside them
# (ADR-041), the type database (ADR-037) in share\mime, and in licenses
# skwir's licence, its crates' notices (tools/package/notices/windows),
# PDFium's and MinGW-w64's.
#
#     tools/package/windows.sh [version]      # into target/package/windows/
set -eu
if ! command -v wixl >/dev/null || ! command -v llvm-rc >/dev/null || ! command -v wrestool >/dev/null; then
    exec nix develop "$(cd "$(dirname "$0")/../.." && pwd)#windows" --command "$0" "$@"
fi
root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"
version="${1:-$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)}"
target=x86_64-pc-windows-gnu
programs="$(grep -v '^#' tools/package/windows/programs)"

# As `just check-windows` builds, but in a folder of its own: its flags
# below would have each rebuild what the other built.
export CARGO_TARGET_DIR="$root/target/windows" CARGO_INCREMENTAL=0
# What a program says of where it was built (a panic's location, C's
# __FILE__) names no folder of this machine: the crates' sources are under
# /cargo, Rust's own under /rust, the tree under /skwir.
cargo_home="${CARGO_HOME:-$HOME/.cargo}"
sysroot="$(rustc --print sysroot)"
CARGO_ENCODED_RUSTFLAGS="$(printf -- '--remap-path-prefix=%s=/cargo\037--remap-path-prefix=%s=/rust\037--remap-path-prefix=%s=/skwir' \
    "$cargo_home" "$sysroot" "$root")"
export CARGO_ENCODED_RUSTFLAGS
# Zig's clang refuses __DATE__ and __TIME__, which mimalloc reads, for the
# sake of reproducible builds: here a warning.
export CFLAGS_x86_64_pc_windows_gnu="-Wno-error=date-time -ffile-prefix-map=$cargo_home=/cargo -ffile-prefix-map=$root=/skwir"
# The resource compiler that embeds skwir.exe's icon (crates/skwir/build.rs)
# and gpui's manifest (vendor patch 0027).
RC_x86_64_pc_windows_gnu="$(command -v llvm-rc)"
export RC_x86_64_pc_windows_gnu
# shellcheck disable=SC2046,SC2086 # one `-p name` per program
cargo-zigbuild build --profile dist --target "$target" $(printf -- '-p %s ' $programs)

name="skwir-$version-windows-x86_64"
out="$root/target/package/windows"
folder="$out/$name"
rm -rf "${folder:?}" "$out/$name.zip" "$out/$name.msi" "$out/work"
mkdir -p "$folder/licenses/mingw-w64" "$out/work"
for program in $programs; do
    cp "$CARGO_TARGET_DIR/$target/dist/$program.exe" "$folder/"
done
tools/package/pdfium.sh win-x64 "$out/work/pdfium"
cp "$out/work/pdfium/pdfium.dll" "$folder/"
cp -R "$out/work/pdfium/licenses/pdfium" "$folder/licenses/"
cp LICENSE "$folder/licenses/"
cp tools/package/notices/windows/skwir/THIRD-PARTY-NOTICES "$folder/licenses/"
# MinGW-w64's runtime, built into each program by Zig, asks for its notice
# in a binary's distribution (ZPL 2.1).
lib="$(zig env | sed -n 's/^ *\.lib_dir = "\(.*\)",$/\1/p')"
cp "$lib/libc/mingw/COPYING" "$folder/licenses/mingw-w64/"
tools/package/mime-database.sh "$folder/share/mime" >/dev/null

# The zip: one folder, its entries sorted and dated by the commit.
python3 - "$out" "$name" "$(git log -1 --format=%ct)" <<'EOF'
import os, pathlib, sys, time, zipfile
out, name, stamp = pathlib.Path(sys.argv[1]), sys.argv[2], int(sys.argv[3])
when = time.gmtime(max(stamp, 315532800))[:6]
with zipfile.ZipFile(out / f"{name}.zip", "w", zipfile.ZIP_DEFLATED, compresslevel=9) as zipped:
    for path in sorted((out / name).rglob("*")):
        if path.is_file():
            entry = zipfile.ZipInfo(str(path.relative_to(out)), when)
            entry.compress_type = zipfile.ZIP_DEFLATED
            entry.external_attr = 0o644 << 16
            zipped.writestr(entry, path.read_bytes())
EOF

# The installer: every file of the folder (wixl-heat), under skwir.wxs's
# folder, shortcut and upgrade, with the icon skwir.exe carries (its
# group icon, resource type 14).
find "$folder" -type f | LC_ALL=C sort | wixl-heat -p "$folder/" --directory-ref INSTALLDIR \
    --component-group Files --var var.Dist --win64 > "$out/work/files.wxs"
wrestool -x -t 14 "$folder/skwir.exe" > "$out/work/skwir.ico"
wixl -a x64 -D "Version=$version" -D "Dist=$folder" -D "Icon=$out/work/skwir.ico" -D Win64=yes \
    -o "$out/$name.msi" \
    tools/package/windows/skwir.wxs "$out/work/files.wxs"
rm -rf "${out:?}/work"

for artifact in "$name.zip" "$name.msi"; do
    (cd "$out" && sha256sum "$artifact" > "$artifact.sha256")
    echo "made: $out/$artifact"
done
