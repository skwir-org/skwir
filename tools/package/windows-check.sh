#!/bin/sh
# What tools/package/windows.sh made, tried under Wine (ADR-074): the zip
# unpacked into a folder whose name has a space and an apostrophe, each
# program answering (`--version`, a helper's `--probe`: PDFium loaded from
# beside it), every file it should hold; then the installer, per user,
# quietly: the programs where it puts them, its Start menu entry, and
# nothing of them left once it is removed.
#
# Wine is not Windows. It cannot draw skwir's window (its DirectWrite has
# no font sets; Direct Manipulation and DirectComposition are missing), so
# the window is for people to try (docs/HELP-WANTED.md). Its prefix is this
# script's own; a stand-in for Windows' icuuc.dll goes in it
# (windows/icuuc-for-wine.c).
#
#     tools/package/windows-check.sh [version]
set -eu
if ! command -v wine >/dev/null; then
    exec nix develop "$(cd "$(dirname "$0")/../.." && pwd)#windows" --command "$0" "$@"
fi
root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"
version="${1:-$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)}"
name="skwir-$version-windows-x86_64"
made="$root/target/package/windows"
programs="$(grep -v '^#' tools/package/windows/programs)"
for artifact in "$name.zip" "$name.msi"; do
    (cd "$made" && sha256sum --quiet -c "$artifact.sha256")
done

scratch="$HOME/.cache/skwir-tests/windows-check"
rm -rf "${scratch:?}"
mkdir -p "$scratch"
export WINEPREFIX="$HOME/.cache/skwir-tests/wine/package-prefix" WINEDEBUG=-all
# No Mono or Gecko: their installers would ask on first start.
export WINEDLLOVERRIDES="mscoree,mshtml="
# The dev shell's, for Linux: a Windows program must not see them.
unset SKWIR_PDFIUM_LIBRARY XDG_DATA_DIRS DISPLAY WAYLAND_DISPLAY
trap 'wineserver -k 2>/dev/null || true' EXIT
wineboot -i >/dev/null 2>&1 || true
system32="$WINEPREFIX/drive_c/windows/system32"
zig cc -target x86_64-windows-gnu -shared -O2 -o "$scratch/icuuc.dll" tools/package/windows/icuuc-for-wine.c
cp "$scratch/icuuc.dll" "$system32/icuuc.dll"

# Its programs, as found in `folder`.
answer() {
    folder="$1"
    said="$(wine "$folder/skwir.exe" --version 2>/dev/null | tr -d '\r')"
    case "$said" in
    *"$version"*) echo "ok: skwir.exe --version: $said" ;;
    *) echo "skwir.exe --version said: $said" >&2; exit 1 ;;
    esac
    for program in $programs; do
        [ "$program" = skwir ] && continue
        wine "$folder/$program.exe" --probe >/dev/null 2>&1 || { echo "$program.exe --probe failed" >&2; exit 1; }
        echo "ok: $program.exe --probe"
    done
}

# The zip: one folder, the files listed, and nothing else at its top.
unpacked="$scratch/a traveller's programs"
# shellcheck disable=SC2086 # one argument per program
python3 - "$made/$name.zip" "$unpacked" "$name" $programs <<'EOF'
import pathlib, sys, zipfile
archive, into, name, programs = sys.argv[1], pathlib.Path(sys.argv[2]), sys.argv[3], sys.argv[4:]
with zipfile.ZipFile(archive) as zipped:
    names = zipped.namelist()
    assert {n.split("/")[0] for n in names} == {name}, "the zip holds one folder"
    zipped.extractall(into)
folder = into / name
wanted = [f"{p}.exe" for p in programs] + [
    "pdfium.dll", "share/mime/globs2", "share/mime/magic", "share/mime/COPYING",
    "licenses/LICENSE", "licenses/THIRD-PARTY-NOTICES", "licenses/mingw-w64/COPYING",
    "licenses/pdfium/LICENSE"]
missing = [w for w in wanted if not (folder / w).is_file()]
assert not missing, f"missing from the zip: {missing}"
top = {p.name for p in folder.iterdir()}
assert top == {f"{p}.exe" for p in programs} | {"pdfium.dll", "share", "licenses"}, top
print(f"ok: the zip holds {len(names)} files in {name}/")
EOF
answer "$unpacked/$name"
# Nothing names a folder of the machine that built it (windows.sh's
# remapping).
for program in $programs; do
    if strings "$unpacked/$name/$program.exe" | grep -qE "$HOME|/nix/store/[a-z0-9]{32}-"; then
        echo "$program.exe names a folder of the machine that built it" >&2
        exit 1
    fi
done
echo "ok: no program names a folder of the machine that built it"

# The installer, per user, then removed.
user="$(wine cmd /c 'echo %USERNAME%' 2>/dev/null | tr -d '\r')"
home="$WINEPREFIX/drive_c/users/$user"
installed="$home/AppData/Local/Programs/skwir"
shortcut="$home/AppData/Roaming/Microsoft/Windows/Start Menu/Programs/skwir.lnk"
wine msiexec /i "$made/$name.msi" /qn
[ -f "$installed/skwir.exe" ] && [ -f "$installed/licenses/THIRD-PARTY-NOTICES" ] \
    && [ -f "$installed/share/mime/globs2" ] || { echo "the installer put nothing in $installed" >&2; exit 1; }
echo "ok: installed in %LocalAppData%\\Programs\\skwir, $(find "$installed" -type f | wc -l) files"
[ -f "$shortcut" ] || { echo "no Start menu entry: $shortcut" >&2; exit 1; }
echo "ok: a Start menu entry"
answer "$installed"
wine msiexec /x "$made/$name.msi" /qn
left="$(find "$installed" -type f 2>/dev/null | wc -l)"
[ "$left" -eq 0 ] && [ ! -e "$shortcut" ] || { echo "removed, $left files and the entry left" >&2; exit 1; }
echo "ok: removed, nothing of it left"
echo "$name: passed under Wine"
