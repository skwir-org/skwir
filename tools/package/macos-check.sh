#!/bin/sh
# What tools/package/macos.sh made, looked over on Linux (ADR-075). Nothing
# here runs a Mac's program, so this reads what can be read: the zip holds
# skwir.app alone, its programs executable, and the disk image the same
# bundle, file for file, beside a link to Applications; Info.plist names
# what is there, and the languages skwir speaks, an .lproj each;
# the icon holds every side; each program is arm64, for macOS 12 or later,
# linked to the system's libraries alone, skwird's beside them, from its
# release; the bundle's seal verifies (rcodesign); no program names a
# folder of the machine that built it.
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

# The zip, unpacked as a Mac's Archive Utility would, modes and links
# kept; its Info.plist and icon read.
# shellcheck disable=SC2086 # one argument per program
python3 - "$made/$name.zip" "$unpacked" "$version" $programs <<'EOF'
import pathlib, plistlib, re, stat, struct, sys, zipfile
archive, into, version, programs = sys.argv[1], pathlib.Path(sys.argv[2]), sys.argv[3], sys.argv[4:]
with zipfile.ZipFile(archive) as zipped:
    entries = zipped.infolist()
    assert {e.filename.split("/")[0] for e in entries} == {"skwir.app"}, "the zip holds skwir.app alone"
    for entry in entries:
        mode, target = entry.external_attr >> 16, into / entry.filename
        if stat.S_ISLNK(mode):
            target.parent.mkdir(parents=True, exist_ok=True)
            target.symlink_to(zipped.read(entry).decode())
        else:
            zipped.extract(entry, into)
            target.chmod(mode & 0o777)
app = into / "skwir.app/Contents"
for program in [*programs, "skwird"]:
    path = app / "MacOS" / program
    assert path.is_file() and path.stat().st_mode & 0o111, f"{program} missing or not executable"
for wanted in ["Frameworks/libpdfium.dylib", "Resources/skwir.icns", "Resources/share/mime/globs2",
               "Resources/share/mime/magic", "Resources/licenses/LICENSE",
               "Resources/licenses/THIRD-PARTY-NOTICES", "Resources/licenses/pdfium/LICENSE",
               "Resources/share/skwir/channel",
               "Resources/share/skwir/fonts/Inter-Regular.ttf",
               "Resources/share/skwir/fonts/Inter-Bold.ttf",
               "Resources/share/skwir/fonts/JetBrainsMono-Regular.ttf",
               "Resources/licenses/fonts/Inter/LICENSE.txt",
               "Resources/licenses/fonts/JetBrainsMono/OFL.txt",
               "Resources/licenses/skwird/LICENSE-MIT", "Resources/licenses/skwird/LICENSE-APACHE",
               "Resources/licenses/skwird/THIRD-PARTY-NOTICES",
               "Resources/licenses/libarchive/COPYING", "Resources/licenses/libarchive/VERSIONS",
               "Resources/licenses/zstd/LICENSE", "_CodeSignature/CodeResources",
               "Frameworks/GStreamer.framework/Versions/1.0/lib/libgstreamer-1.0.0.dylib",
               "Frameworks/GStreamer.framework/Versions/1.0/lib/gstreamer-1.0/libgstplayback.dylib",
               "Frameworks/GStreamer.framework/Versions/1.0/lib/gstreamer-1.0/libgstlibav.dylib",
               "Frameworks/GStreamer.framework/Versions/1.0/lib/gstreamer-1.0/libgstvideofilter.dylib",
               "Frameworks/GStreamer.framework/Versions/1.0/libexec/gstreamer-1.0/gst-plugin-scanner",
               "Frameworks/GStreamer.framework/Resources/Info.plist",
               "Frameworks/GStreamer.framework/GStreamer",
               "Resources/licenses/gstreamer/VERSIONS", "Resources/licenses/gstreamer/SOURCES"]:
    assert (app / wanted).is_file(), f"missing: {wanted}"
# How the copy came (ADR-142).
assert (app / "Resources/share/skwir/channel").read_text() == "macos\n", "the app's channel"
info = plistlib.loads((app / "Info.plist").read_bytes())
assert info["CFBundleExecutable"] == "skwir" and info["CFBundleIdentifier"] == "org.skwir.skwir", info
assert info["CFBundleShortVersionString"] == version and info["LSMinimumSystemVersion"] == "12.0", info
# English and each language skwir offers, reviewed or as a draft (ADR-139,
# 13, tools/words/offered: a line's first word is its tag), declared, each
# with its .lproj saying skwir's name.
offered = pathlib.Path("tools/words/offered").read_text().splitlines()
languages = ["en", *sorted({line.split()[0] for line in offered if line.strip() and not line.startswith("#")})]
assert info["CFBundleDevelopmentRegion"] == "en" and info["CFBundleLocalizations"] == languages, info
for language in languages:
    strings = (app / "Resources" / f"{language}.lproj/InfoPlist.strings").read_text()
    said = dict(re.findall(r'^(\w+) = "([^"]*)";$', strings, re.M))
    assert said == {"CFBundleName": "skwir", "CFBundleDisplayName": "skwir"}, (language, said)
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
print(f"ok: the zip holds skwir.app, {len(entries)} files; Info.plist, its icon ({len(kinds)} sides) "
      f"and its languages ({', '.join(languages)}) read")
EOF

# The disk image, opened with the tools that made it: the same bundle, each
# file's bytes and mode and each link, and the link to Applications.
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
    return {str(p.relative_to(root)): ("link", os.readlink(p)) if p.is_symlink() else
            (p.stat().st_mode & 0o111 != 0, hashlib.sha256(p.read_bytes()).hexdigest())
            for p in root.rglob("*") if p.is_symlink() or p.is_file()}
ours, theirs = tree(zipped), tree(volume / "skwir.app")
assert ours == theirs, f"the disk image's bundle differs: {sorted(set(ours.items()) ^ set(theirs.items()))[:5]}"
print(f"ok: the disk image holds the same skwir.app ({len(ours)} files, modes kept) beside a link to Applications")
EOF

app="$unpacked/skwir.app/Contents"
for program in $programs; do
    binary="$app/MacOS/$program"
    file -b "$binary" | grep -q '^Mach-O 64-bit arm64 executable' || { echo "$program: not an arm64 program" >&2; exit 1; }
    minos="$(llvm-otool -l "$binary" | sed -n '/LC_BUILD_VERSION/,/ntools/s/^ *minos //p')"
    [ "$minos" = 12.0 ] || { echo "$program: for macOS $minos" >&2; exit 1; }
    # The system's libraries, or GStreamer's in the bundle (by @rpath).
    foreign=""
    for library in $(llvm-otool -L "$binary" | tail -n +2 | awk '{print $1}'); do
        case "$library" in
        /usr/lib/* | /System/Library/*) ;;
        @rpath/*) [ -f "$app/Frameworks/GStreamer.framework/Versions/1.0/lib/${library#@rpath/}" ] || foreign="$foreign $library" ;;
        *) foreign="$foreign $library" ;;
        esac
    done
    [ -z "$foreign" ] || { echo "$program links$foreign" >&2; exit 1; }
    if strings "$binary" | grep -qE "$HOME|/nix/store/[a-z0-9]{32}-"; then
        echo "$program names a folder of the machine that built it" >&2
        exit 1
    fi
    echo "ok: $program: arm64, macOS 12 or later, the system's libraries (and the bundle's GStreamer) alone, no build folder"
done
# skwird, carried beside skwir (tools/package/skwird.py): its release's
# program, for Apple silicon and macOS 11 or later (skwird's own floor,
# below skwir's), linked to the system's libraries alone, signed again
# with the bundle (the seal below) and so no longer bearing its archive's
# signature; its licences are its release's.
python3 tools/package/skwird.py macos "$scratch/skwird"
binary="$app/MacOS/skwird"
file -b "$binary" | grep -q '^Mach-O 64-bit arm64 executable' || { echo "skwird: not an arm64 program" >&2; exit 1; }
minos="$(llvm-otool -l "$binary" | sed -n '/LC_BUILD_VERSION/,/ntools/s/^ *minos //p')"
case "$minos" in
11.* | 12.0) ;;
*) echo "skwird: for macOS $minos, after skwir's 12.0" >&2; exit 1 ;;
esac
for library in $(llvm-otool -L "$binary" | tail -n +2 | awk '{print $1}'); do
    case "$library" in
    /usr/lib/* | /System/Library/*) ;;
    *) echo "skwird links $library" >&2; exit 1 ;;
    esac
done
if strings "$binary" | grep -qE "$HOME|/nix/store/[a-z0-9]{32}-"; then
    echo "skwird names a folder of the machine that built it" >&2
    exit 1
fi
for licence in LICENSE-MIT LICENSE-APACHE THIRD-PARTY-NOTICES; do
    cmp -s "$scratch/skwird/licenses/skwird/$licence" "$app/Resources/licenses/skwird/$licence" \
        || { echo "Resources/licenses/skwird/$licence is not skwird's" >&2; exit 1; }
done
if cmp -s "$scratch/skwird/skwird" "$binary"; then
    echo "skwird bears its archive's signature: the bundle's signing left it" >&2
    exit 1
fi
echo "ok: skwird $(sed -n 's/^release v//p' tools/package/skwird.lock): arm64, macOS $minos or later, the system's libraries alone, no build folder, signed again with the bundle, its licences beside"
# GStreamer in the bundle (tools/package/gstreamer.py, ADR-076), a
# framework whose main binary is libgstreamer: each library, plugin and
# the scanner Apple silicon's alone, and what each loads the system's or
# GStreamer's own beside it.
python3 - "$app/Frameworks/GStreamer.framework" <<'EOF'
import os, pathlib, plistlib, subprocess, sys
framework = pathlib.Path(sys.argv[1])
root = framework / "Versions/1.0"
info = plistlib.loads((framework / "Resources/Info.plist").read_bytes())
assert info["CFBundleExecutable"] == "GStreamer" and info["CFBundlePackageType"] == "FMWK", info
# libgstreamer is the main binary, a file, and lib/ names it by a link
# (the seal below says why).
assert (root / "GStreamer").is_file() and not (root / "GStreamer").is_symlink(), "the framework's main binary"
assert os.readlink(root / "lib/libgstreamer-1.0.0.dylib") == "../GStreamer", "libgstreamer, named in lib/"
macho = lambda p: p.read_bytes()[:4] in (b"\xcf\xfa\xed\xfe", b"\xca\xfe\xba\xbe")
files = sorted(p for p in root.rglob("*") if p.is_file() and not p.is_symlink() and macho(p))
for path in files:
    archs = subprocess.run(["llvm-lipo", "-archs", str(path)], capture_output=True, text=True).stdout.split()
    assert archs == ["arm64"], f"{path.relative_to(root)}: {archs}"
    said = subprocess.run(["llvm-otool", "-L", str(path)], check=True, capture_output=True, text=True).stdout
    for library in [line.split()[0] for line in said.splitlines()[1:] if line.strip()]:
        name = library.removeprefix("@rpath/")
        if library.startswith(("/usr/lib/", "/System/Library/")) or name == path.name:
            continue
        assert library.startswith("@rpath/") and (root / "lib" / name).is_file(), \
            f"{path.relative_to(root)} loads {library}"
print(f"ok: GStreamer in the bundle, a framework of {len(files)} programs and libraries: arm64, loading the system's or its own")
EOF
# The archive helper carries its libarchive (tools/package/libarchive.sh):
# built without it, it would link the system's, whose formats vary.
if llvm-otool -L "$app/MacOS/skwir-archive-worker" | grep -q libarchive; then
    echo "skwir-archive-worker links the system's libarchive, not its own" >&2
    exit 1
fi
echo "ok: skwir-archive-worker carries its own libarchive"
# The seals, checked here as macOS checks them (`codesign --verify --deep
# --strict`; rcodesign's `verify` reads no bundle): the app and each bundle
# it holds (GStreamer's framework). A bundle's main program binds its
# Info.plist and its list of resources; that list holds each file's
# digest, each link's target (relative, leading inside that bundle) and
# each program or bundle nested in it by its code's hash; nothing else is
# there. A main program that is a link
# fails it: rcodesign seals what the link names as a resource, then signs
# it again as the main program (every Mac build to 0.7.0, refused by a
# Mac on 2026-11-25).
python3 - "$app" <<'EOF'
import hashlib, os, pathlib, plistlib, re, subprocess, sys
app = pathlib.Path(sys.argv[1])
macho = lambda p: p.read_bytes()[:4] in (b"\xcf\xfa\xed\xfe", b"\xca\xfe\xba\xbe")
codes = [p for p in [*(app / "MacOS").rglob("*"), *(app / "Frameworks").rglob("*")]
         if p.is_file() and not p.is_symlink() and macho(p)]
signatures = {}
def signature(code):
    code = code.resolve()
    if code not in signatures:
        signatures[code] = subprocess.run(["rcodesign", "print-signature-info", str(code)],
                                          check=True, capture_output=True, text=True).stdout
    return signatures[code]
for code in codes:
    assert "CodeSignatureFlags(ADHOC)" in signature(code), f"{code.relative_to(app)}: not signed"
# The hash a seal names a nested program by: its code directory's digest, cut to 20 bytes.
def cdhash(code):
    found = re.search(r"slot: CodeDirectory \(0\)\n(?:[ \t]+\w+: .*\n)*?[ \t]+sha256: ([0-9a-f]{64})", signature(code))
    assert found, f"{code.relative_to(app)}: no code directory"
    return bytes.fromhex(found[1])[:20]
sha = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
def bundle(root):
    """A bundle's Info.plist and its main program: an app's in MacOS, a framework's beside it."""
    plist = root / "Info.plist" if (root / "Info.plist").exists() else root / "Resources/Info.plist"
    executable = plistlib.loads(plist.read_bytes())["CFBundleExecutable"]
    return plist, (root / "MacOS" / executable if (root / "MacOS").is_dir() else root / executable)
def nested(path):
    """What a seal's nested entry names: a program, or a bundle by its main program."""
    if path.is_dir():
        for current in (path / "Versions/Current", path):
            if (current / "_CodeSignature/CodeResources").exists():
                return bundle(current)[1]
    return path
roots = [app, *sorted(p.parent.parent for p in (app / "Frameworks").rglob("_CodeSignature/CodeResources"))]
checked = 0
for root in roots:
    where = "the app" if root == app else str(root.relative_to(app))
    plist, main_path = bundle(root)
    assert not main_path.is_symlink(), \
        f"{main_path.relative_to(app)} is a link: rcodesign signs what it names again once that is sealed"
    main = signature(main_path)
    slots = dict(re.findall(r"'(Info|Resources) \(\d\): ([0-9a-f]{64})'", main))
    resources = root / "_CodeSignature/CodeResources"
    assert slots.get("Info") == sha(plist), f"{where}: its Info.plist is not the one signed"
    assert slots.get("Resources") == sha(resources), f"{where}: its list of resources is not the one signed"
    listed = plistlib.loads(resources.read_bytes())["files2"]
    for path, entry in listed.items():
        target = root / path
        if not isinstance(entry, dict):
            entry = {"hash": entry}
        if entry.get("optional") and not os.path.lexists(target):
            continue
        if "symlink" in entry:
            assert target.is_symlink() and os.readlink(target) == entry["symlink"], f"{where}: {path} is not the link sealed"
            # And it leads, relative, to what is there inside the bundle that
            # seals it: `codesign --strict` refuses a link out of its bundle,
            # which on another Mac names something else or nothing.
            assert not os.path.isabs(entry["symlink"]), f"{where}: {path} is an absolute link"
            led = pathlib.Path(os.path.realpath(target))
            assert led.exists() and led.is_relative_to(root.resolve()), \
                f"{where}: {path} leads out of its bundle, to {led}"
        elif "cdhash" in entry:
            assert cdhash(nested(target)) == entry["cdhash"], f"{where}: {path} is not the code sealed"
        else:
            assert not target.is_symlink() and target.is_file(), f"{where}: {path}, sealed, is missing"
            assert hashlib.sha256(target.read_bytes()).digest() == entry["hash2"], f"{where}: {path} changed since sealed"
        checked += 1
    # Strict: nothing there that the seal does not name, but what it leaves
    # out (the main program, the list itself, an app's Info.plist) and what
    # is inside what it names as nested.
    inside = [path for path, entry in listed.items() if isinstance(entry, dict) and "cdhash" in entry]
    for path in root.rglob("*"):
        relative = str(path.relative_to(root))
        if path.is_dir() and not path.is_symlink() or relative in listed:
            continue
        if path == main_path or relative.startswith("_CodeSignature/") or relative == "Info.plist" \
                or any(relative.startswith(n + "/") for n in inside) or path.name == ".DS_Store":
            continue
        raise AssertionError(f"{where}: {relative} is there but not sealed")
print(f"ok: signed ad hoc; {len(roots)} bundles' seals whole ({checked} entries), each of {len(codes)} programs and libraries signed")
EOF
echo "$name: looked over (not run: that needs a Mac)"
# What passed, where a release collects it (SKWIR_PACKAGE_OUT, as try.sh).
if [ -n "${SKWIR_PACKAGE_OUT:-}" ]; then
    for artifact in "$name.zip" "$name.dmg"; do
        cp "$made/$artifact" "$made/$artifact.sha256" "$SKWIR_PACKAGE_OUT/"
    done
fi
