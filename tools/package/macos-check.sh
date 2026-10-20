#!/bin/sh
# What tools/package/macos.sh made, looked over on Linux (ADR-075). Nothing
# here runs a Mac's program, so this reads what can be read: the zip holds
# skwir.app alone, its programs executable, and the disk image the same
# bundle, file for file, beside a link to Applications; Info.plist names
# what is there;
# the icon holds every side; each program is arm64, for macOS 11 or later,
# linked to the system's libraries alone; the bundle's seal verifies
# (rcodesign); no program names a folder of the machine that built it.
# Whether it opens is for people on a Mac (docs/HELP-WANTED.md).
#
#     tools/package/macos-check.sh [version]
set -eu
if ! command -v rcodesign >/dev/null || ! command -v llvm-otool >/dev/null || ! command -v hfsplus >/dev/null; then
    exec nix develop "$(cd "$(dirname "$0")/../.." && pwd)#macos" --command "$0" "$@"
fi
root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"
version="${1:-$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)}"
name="skwir-$version-macos-arm64"
made="$root/target/package/macos"
programs="$(grep -v '^#' tools/package/macos/programs)"
for artifact in "$name.zip" "$name.dmg"; do
    (cd "$made" && sha256sum --quiet -c "$artifact.sha256")
done

scratch="$HOME/.cache/skwir-tests/macos-check"
rm -rf "${scratch:?}"
unpacked="$scratch/a traveller's apps"
mkdir -p "$unpacked"

# The zip, unpacked as a Mac's Archive Utility would, modes kept; its
# Info.plist and icon read.
# shellcheck disable=SC2086 # one argument per program
python3 - "$made/$name.zip" "$unpacked" "$version" $programs <<'EOF'
import pathlib, plistlib, struct, sys, zipfile
archive, into, version, programs = sys.argv[1], pathlib.Path(sys.argv[2]), sys.argv[3], sys.argv[4:]
with zipfile.ZipFile(archive) as zipped:
    entries = zipped.infolist()
    assert {e.filename.split("/")[0] for e in entries} == {"skwir.app"}, "the zip holds skwir.app alone"
    for entry in entries:
        zipped.extract(entry, into)
        (into / entry.filename).chmod((entry.external_attr >> 16) & 0o777)
app = into / "skwir.app/Contents"
for program in programs:
    path = app / "MacOS" / program
    assert path.is_file() and path.stat().st_mode & 0o111, f"{program} missing or not executable"
for wanted in ["Frameworks/libpdfium.dylib", "Resources/skwir.icns", "Resources/share/mime/globs2",
               "Resources/share/mime/magic", "Resources/licenses/LICENSE",
               "Resources/licenses/THIRD-PARTY-NOTICES", "Resources/licenses/pdfium/LICENSE",
               "_CodeSignature/CodeResources"]:
    assert (app / wanted).is_file(), f"missing: {wanted}"
info = plistlib.loads((app / "Info.plist").read_bytes())
assert info["CFBundleExecutable"] == "skwir" and info["CFBundleIdentifier"] == "org.skwir.skwir", info
assert info["CFBundleShortVersionString"] == version and info["LSMinimumSystemVersion"] == "11.0", info
icns = (app / "Resources" / f'{info["CFBundleIconFile"]}.icns').read_bytes()
assert icns[:4] == b"icns" and struct.unpack(">I", icns[4:8])[0] == len(icns), "not an .icns"
kinds, at = set(), 8
while at < len(icns):
    kind, size = icns[at:at + 4].decode(), struct.unpack(">I", icns[at + 4:at + 8])[0]
    assert icns[at + 8:at + 16] == b"\x89PNG\r\n\x1a\n", f"{kind}: not a PNG"
    kinds.add(kind)
    at += size
missing = {"icp4", "icp5", "icp6", "ic07", "ic08", "ic09", "ic10", "ic11", "ic12", "ic13", "ic14"} - kinds
assert not missing, f"the icon lacks {missing}"
print(f"ok: the zip holds skwir.app, {len(entries)} files; Info.plist and its icon ({len(kinds)} sides) read")
EOF

# The disk image, opened with the tools that made it: the same bundle, each
# file's bytes and mode, and the link to Applications.
volume="$scratch/from the disk image"
mkdir -p "$volume"
dmg extract "$made/$name.dmg" "$scratch/skwir.hfs" >/dev/null
hfsplus "$scratch/skwir.hfs" extractall / "$volume/" >/dev/null
python3 - "$unpacked/skwir.app" "$volume" <<'EOF'
import hashlib, os, pathlib, sys
zipped, volume = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2])
assert {p.name for p in volume.iterdir()} == {"skwir.app", "Applications"}, sorted(volume.iterdir())
assert os.readlink(volume / "Applications") == "/Applications", "no link to Applications"
def tree(root):
    return {str(p.relative_to(root)): (p.stat().st_mode & 0o111 != 0, hashlib.sha256(p.read_bytes()).hexdigest())
            for p in root.rglob("*") if p.is_file()}
ours, theirs = tree(zipped), tree(volume / "skwir.app")
assert ours == theirs, f"the disk image's bundle differs: {sorted(set(ours.items()) ^ set(theirs.items()))[:5]}"
print(f"ok: the disk image holds the same skwir.app ({len(ours)} files, modes kept) beside a link to Applications")
EOF

app="$unpacked/skwir.app/Contents"
for program in $programs; do
    binary="$app/MacOS/$program"
    file -b "$binary" | grep -q '^Mach-O 64-bit arm64 executable' || { echo "$program: not an arm64 program" >&2; exit 1; }
    minos="$(llvm-otool -l "$binary" | sed -n '/LC_BUILD_VERSION/,/ntools/s/^ *minos //p')"
    [ "$minos" = 11.0 ] || { echo "$program: for macOS $minos" >&2; exit 1; }
    foreign="$(llvm-otool -L "$binary" | tail -n +2 | awk '{print $1}' | grep -vE '^/usr/lib/|^/System/Library/' || true)"
    [ -z "$foreign" ] || { echo "$program links $foreign" >&2; exit 1; }
    if strings "$binary" | grep -qE "$HOME|/nix/store/[a-z0-9]{32}-"; then
        echo "$program names a folder of the machine that built it" >&2
        exit 1
    fi
    echo "ok: $program: arm64, macOS 11 or later, the system's libraries alone, no build folder"
done
# The seal, checked here as macOS checks it (rcodesign's `verify` reads no
# bundle): the main program's signature binds Info.plist and the list of
# resources; that list holds each file's digest, and each program and
# PDFium is signed in turn.
for code in "$app"/MacOS/* "$app"/Frameworks/*; do
    rcodesign print-signature-info "$code" > "$scratch/$(basename "$code").signature"
done
python3 - "$app" "$scratch" <<'EOF'
import base64, hashlib, pathlib, plistlib, re, sys
app, scratch = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2])
for code in [*(app / "MacOS").iterdir(), *(app / "Frameworks").iterdir()]:
    info = (scratch / f"{code.name}.signature").read_text()
    assert "CodeSignatureFlags(ADHOC)" in info, f"{code.name}: not signed"
main = (scratch / "skwir.signature").read_text()
slots = dict(re.findall(r"'(Info|Resources) \(\d\): ([0-9a-f]{64})'", main))
sha = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
assert "identifier: org.skwir.skwir" in main, "the main program is not org.skwir.skwir"
assert slots.get("Info") == sha(app / "Info.plist"), "Info.plist is not the one signed"
resources = app / "_CodeSignature/CodeResources"
assert slots.get("Resources") == sha(resources), "the resources' list is not the one signed"
listed = plistlib.loads(resources.read_bytes())["files2"]
checked = 0
for path, entry in listed.items():
    if isinstance(entry, dict) and "hash2" in entry:
        assert entry["hash2"] == hashlib.sha256((app / path).read_bytes()).digest(), f"{path} changed"
        checked += 1
print(f"ok: signed ad hoc; Info.plist and {checked} resources as sealed, each program signed")
EOF
echo "$name: looked over (not run: that needs a Mac)"
# What passed, where a release collects it (SKWIR_PACKAGE_OUT, as try.sh).
if [ -n "${SKWIR_PACKAGE_OUT:-}" ]; then
    for artifact in "$name.zip" "$name.dmg"; do
        cp "$made/$artifact" "$made/$artifact.sha256" "$SKWIR_PACKAGE_OUT/"
    done
fi
