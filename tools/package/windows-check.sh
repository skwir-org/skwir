#!/bin/sh
# What tools/package/windows.sh made, tried under Wine (ADR-074): the zip
# unpacked into a folder whose name has a space and an apostrophe, each
# program answering (`--version`, a helper's `--probe`: PDFium loaded from
# beside it), every file it should hold; the archive helper making and
# reading archives with the libarchive linked into it; the media helper
# playing clips with the GStreamer beside it (H.264 and AAC by FFmpeg's
# decoders: Wine has no Direct3D or Media Foundation decoders); skwird,
# carried beside skwir, its release's own programs, starting; then the
# installer, per user, quietly: the programs where it puts them, its Start
# menu entry, and nothing of them left once it is removed, the logon task
# skwird registered from the copy it installed taken back first.
#
# Wine is not Windows. It cannot draw skwir's window (its DirectWrite has
# no font sets; Direct Manipulation and DirectComposition are missing), so
# the window is for people to try (docs/HELP-WANTED.md). Its prefix is this
# script's own.
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
export WINEPREFIX="$HOME/.cache/skwir-tests/wine/package-prefix"
. tools/cross/wine-env.sh
# The dev shell's, for Linux: a Windows program must not see them.
unset SKWIR_PDFIUM_LIBRARY XDG_DATA_DIRS DISPLAY WAYLAND_DISPLAY
trap 'wineserver -k 2>/dev/null || true' EXIT
wineboot -i >/dev/null 2>&1 || true

# Where a program is in the folder: beside skwir, but the media helper
# among GStreamer's libraries (as windows.sh puts it).
place() { case "$1" in skwir-media-worker) echo gstreamer/bin ;; *) echo . ;; esac; }

# Its programs, as found in `folder`.
answer() {
    folder="$1"
    said="$(wine "$folder/skwir.exe" --version 2>/dev/null | tr -d '\r')"
    case "$said" in
    *"$version"*) echo "ok: skwir.exe --version: $said" ;;
    *) echo "skwir.exe --version said: $said" >&2; exit 1 ;;
    esac
    # skwird has no --version: a command it does not know, answered with
    # its usage, says it starts.
    said="$(wine "$folder/skwird.exe" help 2>&1 | tr -d '\r' || true)"
    case "$said" in
    "usage: skwird serve"*) echo "ok: skwird.exe starts" ;;
    *) echo "skwird.exe said: $said" >&2; exit 1 ;;
    esac
    for program in $programs; do
        [ "$program" = skwir ] && continue
        wine "$folder/$(place "$program")/$program.exe" --probe >/dev/null 2>&1 || { echo "$program.exe --probe failed" >&2; exit 1; }
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
# The media helper among GStreamer's libraries; the others beside skwir.
at = lambda p: f"gstreamer/bin/{p}.exe" if p == "skwir-media-worker" else f"{p}.exe"
wanted = [at(p) for p in programs] + [
    # skwird (tools/package/skwird.py): its programs, its licences, its crates' notices.
    "skwird.exe", "skwirdw.exe", "licenses/skwird/LICENSE-MIT", "licenses/skwird/LICENSE-APACHE",
    "licenses/skwird/THIRD-PARTY-NOTICES",
    "pdfium.dll", "share/mime/globs2", "share/mime/magic", "share/mime/COPYING",
    "licenses/LICENSE", "licenses/THIRD-PARTY-NOTICES", "licenses/mingw-w64/COPYING",
    "licenses/pdfium/LICENSE", "licenses/libarchive/COPYING", "licenses/libarchive/VERSIONS",
    "share/skwir/channel",
    "share/skwir/fonts/Inter-Regular.ttf", "share/skwir/fonts/Inter-Bold.ttf",
    "share/skwir/fonts/JetBrainsMono-Regular.ttf", "licenses/fonts/Inter/LICENSE.txt",
    "licenses/fonts/JetBrainsMono/OFL.txt",
    "licenses/zstd/LICENSE", "gstreamer/bin/libgstreamer-1.0-0.dll",
    "gstreamer/lib/gstreamer-1.0/libgstplayback.dll", "gstreamer/lib/gstreamer-1.0/libgstlibav.dll",
    "gstreamer/lib/gstreamer-1.0/libgstvideofilter.dll",
    "gstreamer/libexec/gstreamer-1.0/gst-plugin-scanner.exe", "licenses/gstreamer/VERSIONS",
    "licenses/gstreamer/SOURCES", "licenses/gstreamer/ffmpeg/LGPL-2.1-or-later.txt"]
missing = [w for w in wanted if not (folder / w).is_file()]
assert not missing, f"missing from the zip: {missing}"
# How the copy came (ADR-142): the zip's word, not the installer's.
assert (folder / "share/skwir/channel").read_text() == "windows-zip\n", "the zip's channel"
top = {p.name for p in folder.iterdir()}
beside = {f"{p}.exe" for p in programs if "/" not in at(p)}
assert top == beside | {"skwird.exe", "skwirdw.exe", "pdfium.dll", "share", "licenses", "gstreamer"}, top
print(f"ok: the zip holds {len(names)} files in {name}/")
EOF
answer "$unpacked/$name"
# skwird's programs and licences are its release's, byte for byte, the zip
# its sum pinned (tools/package/skwird.lock).
python3 tools/package/skwird.py windows "$scratch/skwird"
for file in skwird.exe skwirdw.exe licenses/skwird/LICENSE-MIT licenses/skwird/LICENSE-APACHE \
    licenses/skwird/THIRD-PARTY-NOTICES; do
    cmp -s "$scratch/skwird/$file" "$unpacked/$name/$file" || { echo "$file is not skwird's release's" >&2; exit 1; }
done
echo "ok: skwird.exe and skwirdw.exe are skwird $(sed -n 's/^release v//p' tools/package/skwird.lock)'s, its licences beside"
# Nothing names a folder of the machine that built it (windows.sh's
# remapping).
for program in $programs; do
    if strings "$unpacked/$name/$(place "$program")/$program.exe" | grep -qE "$HOME|/nix/store/[a-z0-9]{32}-"; then
        echo "$program.exe names a folder of the machine that built it" >&2
        exit 1
    fi
done
echo "ok: no program names a folder of the machine that built it"

# The archive helper, libarchive and its compressors linked in: each
# format skwir makes, of members whose names are not ASCII, made in a
# folder whose name is not either and listed again; a ZIP made here names
# its members in UTF-8, flagged so; and a GNU tar's names, UTF-8 without a
# header saying so, are read as UTF-8 (libarchive's default on Windows is
# the OEM code page).
python3 - "$unpacked/$name/skwir-archive-worker.exe" "$scratch/archives d'été" <<'EOF'
import io, json, pathlib, subprocess, sys, tarfile, zipfile
helper, work = sys.argv[1], pathlib.Path(sys.argv[2])
work.mkdir(parents=True)
def windows(path):
    return subprocess.run(["winepath", "-w", str(path)], check=True, capture_output=True, text=True).stdout.strip()
def run(*args, stdin=b""):
    done = subprocess.run(["wine", helper, *args], input=stdin, capture_output=True)
    assert done.returncode == 0, f"{args[1:]}: {done.stderr.decode(errors='replace')}"
    return done.stdout
names = ["dossier d'essai/", "dossier d'essai/café — note.txt", "日本語.txt"]
wanted = sorted(n.rstrip("/") for n in names)
body = b"bonjour\n" * 1000
def listed(path):
    said = json.loads(run(windows(path), "--members", "100", "--names", "100000"))
    assert said["complete"], said
    return sorted(m["name"].rstrip("/") for m in said["members"])
for kind in ["zip", "7z", "tar", "tar.gz", "tar.bz2", "tar.xz", "tar.zst"]:
    made, stdin = work / f"made.{kind}", b""
    for n in names:
        header = {"name": n.rstrip("/").encode().hex(), "mode": 0o755, "mtime": [1700000000, 0]}
        if n.endswith("/"):
            stdin += json.dumps({**header, "kind": "dir"}).encode() + b"\n"
        else:
            stdin += json.dumps({**header, "kind": "file", "size": len(body)}).encode() + b"\n" + body
    run(windows(made), "--create", kind, stdin=stdin + b'{"end": true}\n')
    assert listed(made) == wanted, (kind, listed(made))
with zipfile.ZipFile(work / "made.zip") as zipped:
    assert sorted(i.filename.rstrip("/") for i in zipped.infolist()) == wanted
    assert all(i.flag_bits & 0x800 for i in zipped.infolist() if not i.filename.isascii()), "unflagged"
print("ok: the archive helper makes and lists ZIP, 7z, tar, tar.gz, tar.bz2, tar.xz and tar.zst; a ZIP's names are flagged UTF-8")
gnu = work / "gnu.tar"
with tarfile.open(gnu, "w", format=tarfile.GNU_FORMAT, encoding="utf-8") as tar:
    for n in names:
        member = tarfile.TarInfo(n.rstrip("/"))
        if n.endswith("/"):
            member.type = tarfile.DIRTYPE
            tar.addfile(member)
        else:
            member.size = len(body)
            tar.addfile(member, io.BytesIO(body))
assert listed(gnu) == wanted, listed(gnu)
print("ok: a GNU tar's names, UTF-8 without a PAX header, read as UTF-8")
EOF

# The media helper, the GStreamer beside it: a clip of two audio tracks and
# subtitles played, sought, its tracks and subtitles changed, its rate,
# its end, and a phone's clip turned upright (tools/media/check.py). H.264
# and AAC, then VP9 and Opus.
for codecs in h264-aac vp9-opus; do
    said="$(python3 tools/media/check.py --wine --codecs "$codecs" --size 1280x720 \
        --worker "$unpacked/$name/gstreamer/bin/skwir-media-worker.exe" --fixtures "$scratch/clips" 2>&1)" || {
        echo "$said" | tail -n 20 >&2
        echo "the media helper could not play $codecs" >&2
        exit 1
    }
    echo "ok: the media helper plays $codecs: $(echo "$said" | tail -n 1)"
done

# The installer's action taking skwird's logon task back as skwir is
# removed (skwir.wxs): skwird.exe, run with `logon remove`, its answer
# ignored, deferred, as the user, in the removal's script but not an
# upgrade's, before the files go.
python3 - "$made/$name.msi" <<'EOF'
import subprocess, sys
msi = sys.argv[1]
def table(name):
    rows = subprocess.run(["msiinfo", "export", msi, name], check=True, capture_output=True, text=True).stdout
    return [row.split("\t") for row in rows.splitlines()[3:]]
actions = {row[0]: row for row in table("CustomAction")}
# 18: a program the package installs (its File key); 64: its answer
# ignored; 1024: deferred; not 2048, so as the user.
assert actions["SkwirdLogonRemove"][1:4] == ["1106", "skwird.exe", "logon remove"], actions
files = {row[0]: row for row in table("File")}
assert files["skwird.exe"][2] == "skwird.exe", files["skwird.exe"]
steps = {row[0]: (row[1], int(row[2])) for row in table("InstallExecuteSequence")}
condition, at = steps["SkwirdLogonRemove"]
assert condition == 'REMOVE="ALL" AND NOT UPGRADINGPRODUCTCODE', condition
assert steps["InstallInitialize"][1] < at < steps["RemoveFiles"][1], (at, steps)
print(f"ok: the installer runs skwird.exe logon remove as it removes skwir, step {at} of its script")
EOF

# The installer, per user, then removed.
user="$(wine cmd /c 'echo %USERNAME%' 2>/dev/null | tr -d '\r')"
home="$WINEPREFIX/drive_c/users/$user"
installed="$home/AppData/Local/Programs/skwir"
shortcut="$home/AppData/Roaming/Microsoft/Windows/Start Menu/Programs/skwir.lnk"
wine msiexec /i "$made/$name.msi" /qn
[ -f "$installed/skwir.exe" ] && [ -f "$installed/licenses/THIRD-PARTY-NOTICES" ] \
    && [ -f "$installed/share/mime/globs2" ] || { echo "the installer put nothing in $installed" >&2; exit 1; }
echo "ok: installed in %LocalAppData%\\Programs\\skwir, $(find "$installed" -type f | wc -l) files"
# How the copy came (ADR-142): the installer's word, not the zip's.
[ "$(cat "$installed/share/skwir/channel")" = windows-installer ] \
    || { echo "the installed copy's channel is not windows-installer" >&2; exit 1; }
echo "ok: the installed copy's channel says windows-installer"
[ -f "$shortcut" ] || { echo "no Start menu entry: $shortcut" >&2; exit 1; }
echo "ok: a Start menu entry"
answer "$installed"
# skwird's logon task, registered from the installed copy, as its user
# would (Wine keeps a task in system32\tasks, its actions left out: Wine
# never runs it). Wine's task service may not answer the first time it
# starts in a prefix.
tasks="$WINEPREFIX/drive_c/windows/system32/tasks"
for attempt in 1 2 3; do
    said="$(wine "$installed/skwird.exe" logon add 2>&1)" && break
    [ "$attempt" -lt 3 ] || { echo "skwird.exe logon add failed under Wine: $said" >&2; exit 1; }
    sleep 2
done
ls "$tasks"/skwird-S-1-* >/dev/null 2>&1 || { echo "no logon task of skwird's in $tasks" >&2; exit 1; }
echo "ok: skwird.exe logon add, from the installed copy, registered its task"
wine msiexec /x "$made/$name.msi" /qn
left="$(find "$installed" -type f 2>/dev/null | wc -l)"
[ "$left" -eq 0 ] && [ ! -e "$shortcut" ] || { echo "removed, $left files and the entry left" >&2; exit 1; }
if ls "$tasks"/skwird-S-1-* >/dev/null 2>&1; then
    echo "removed, skwird's logon task left: it would start nothing at each logon" >&2
    exit 1
fi
echo "ok: removed, nothing of it left, skwird's logon task taken back"
echo "$name: passed under Wine"
# What passed, where a release collects it (SKWIR_PACKAGE_OUT, as try.sh).
if [ -n "${SKWIR_PACKAGE_OUT:-}" ]; then
    for artifact in "$name.zip" "$name.msi"; do
        cp "$made/$artifact" "$made/$artifact.sha256" "$SKWIR_PACKAGE_OUT/"
    done
fi
