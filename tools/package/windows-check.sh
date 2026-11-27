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
# menu entry, and nothing of them left once it is removed, four times:
# with nothing of skwird's registered or running, its actions making no
# folder of skwird's; with the installed copy's engine running, stopped
# first; with another copy of skwird's logon task and engine, both left;
# with the logon task the installed copy registered, taken back. As it
# installs, its search for the skwirdw.exe an upgrade stops first finds
# none where none is, and one put there before, which it does not run,
# nothing being upgraded. Then upgrades, between small installers made
# from skwir.wxs as windows.sh makes the real one (windows/msi.sh): from a
# skwir after 0.7.1 whose engine runs, the newer stopping it before its
# InstallValidate; from 0.7.1, and from a copy whose skwirdw.exe was
# deleted, nothing run.
#
# Wine is not Windows. It cannot draw skwir's window (its DirectWrite has
# no font sets; Direct Manipulation and DirectComposition are missing), so
# the window is for people to try (docs/HELP-WANTED.md). Wine's
# RemoveExistingProducts removes the older package as a plain removal (no
# UPGRADINGPRODUCTCODE), so what the older's own sequence does as it is
# upgraded (its logon task kept) is for people on Windows. Its prefix is
# this script's own, made anew where a run did not finish.
#
#     tools/package/windows-check.sh [version]
set -eu
if ! command -v wine >/dev/null || ! command -v wixl >/dev/null || ! command -v wrestool >/dev/null; then
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
# One wineserver for the whole run, ended with it, and the prefix made or
# brought up to date under it: its services, Wine's task service among
# them, started once. Not `wineboot -i`, which in a prefix already made
# starts a second service manager beside the first, under which the task
# service stops and starts by turns, so that a logon task is registered or
# taken back at one call and not at the next. What a run killed without
# its trap left (systemd-oomd ends a scope so during heavy links) is ended
# first: its server holds the prefix's lock, and a new one would exit
# saying nothing (status 2). The server ends a minute after its last
# program, should this run be killed too, never kept for good.
#
# The prefix is marked while a run is on, the mark taken away once the
# last installer is removed: a run that failed or was killed between an
# install and its removal leaves skwir installed there and its files where
# the next run's search must find none. A prefix found marked is made
# anew.
mkdir -p "$WINEPREFIX"
wineserver -k 2>/dev/null || true
wineserver -w
unfinished="$WINEPREFIX.running"
if [ -e "$unfinished" ]; then
    echo "a run left the prefix unfinished: made anew"
    rm -rf "${WINEPREFIX:?}" "${WINEPREFIX:?}-xdg"
    mkdir -p "$WINEPREFIX" "$XDG_DATA_HOME" "$XDG_CONFIG_HOME"
fi
: >"$unfinished"
wineserver -p60
wineboot -u >/dev/null 2>&1 || true

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
    # skwird (tools/package/skwird.py): its programs, its licences, its notices.
    "skwird.exe", "skwirdw.exe", "licenses/skwird/LICENSE-MIT", "licenses/skwird/LICENSE-APACHE",
    "licenses/skwird/THIRD-PARTY-NOTICES.txt",
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
# skwird's programs, licences and notices are its release's, byte for
# byte, the zip its sum pinned (tools/package/skwird.lock).
python3 tools/package/skwird.py windows "$scratch/skwird"
for file in skwird.exe skwirdw.exe licenses/skwird/LICENSE-MIT licenses/skwird/LICENSE-APACHE \
    licenses/skwird/THIRD-PARTY-NOTICES.txt; do
    cmp -s "$scratch/skwird/$file" "$unpacked/$name/$file" || { echo "$file is not skwird's release's" >&2; exit 1; }
done
echo "ok: skwird.exe and skwirdw.exe are skwird $(sed -n 's/^release v//p' tools/package/skwird.lock)'s, its licences and notices beside"
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

# The installer's actions as skwir is removed (skwir.wxs): skwirdw.exe run
# with `stop` at once, before InstallValidate, as skwir is removed by its
# user or an upgrade; then with `logon remove --if-mine`, deferred, as the
# user, in the removal's script before the files go, but not as an upgrade
# removes it; each answer ignored. And RemoveExistingProducts where
# MajorUpgrade puts it, before the script, so that an upgrade removes the
# older package whole, its engine stopped by its own action, before the
# newer's files are put in place. As it upgrades a skwir after 0.7.1, the
# older copy's skwirdw.exe run with `stop` at once, before this package's
# InstallValidate, found by AppSearch (windows/msi.sh adds the search) in the
# folder the installer puts skwir in.
python3 - "$made/$name.msi" "$version" <<'EOF'
import subprocess, sys
msi, version = sys.argv[1], sys.argv[2]
def table(name):
    rows = subprocess.run(["msiinfo", "export", msi, name], check=True, capture_output=True, text=True).stdout
    return [row.split("\t") for row in rows.splitlines()[3:]]
actions = {row[0]: row for row in table("CustomAction")}
# 18: a program the package installs (its File key); 64: its answer
# ignored; 1024: deferred, and not 2048, so as the user; neither: at once.
assert actions["SkwirdStop"][1:4] == ["82", "skwirdw.exe", "stop"], actions
assert actions["SkwirdLogonRemove"][1:4] == ["1106", "skwirdw.exe", "logon remove --if-mine"], actions
files = {row[0]: row for row in table("File")}
assert files["skwirdw.exe"][2] == "skwirdw.exe", files["skwirdw.exe"]
steps = {row[0]: (row[1], int(row[2])) for row in table("InstallExecuteSequence")}
assert steps["SkwirdLogonRemove"][0] == 'REMOVE="ALL" AND NOT UPGRADINGPRODUCTCODE', steps["SkwirdLogonRemove"]
assert steps["SkwirdStop"][0] == 'REMOVE="ALL"', steps["SkwirdStop"]
# 50: the program a property names, here the one AppSearch found; 64: its
# answer ignored; at once.
assert actions["SkwirdStopOlder"][1:4] == ["114", "INSTALLED_SKWIRDW", "stop"], actions
assert steps["SkwirdStopOlder"][0] == "OLDER_SKWIRD_STOPS AND INSTALLED_SKWIRDW", steps["SkwirdStopOlder"]
# An older skwir after 0.7.1 and before this one, neither included, of the
# code MajorUpgrade removes, detected alone (2).
upgrades = {row[6]: row for row in table("Upgrade")}
assert upgrades["OLDER_SKWIRD_STOPS"][:6] == [upgrades["WIX_UPGRADE_DETECTED"][0], "0.7.1", version, "", "2", ""], \
    upgrades
# skwirdw.exe in INSTALLDIR as the Directory table makes it (searched
# before CostFinalize resolves INSTALLDIR itself), in that folder alone.
folders = {row[0]: row for row in table("Directory")}
def place(folder):
    parent, name = folders[folder][1], folders[folder][2].split(":")[0].split("|")[-1]
    return f"[{folder}]" if name == "." else place(parent) + name + "\\"
assert [tuple(row) for row in table("AppSearch")] == [("INSTALLED_SKWIRDW", "InstalledSkwirdw")], table("AppSearch")
searched = {row[0]: row for row in table("DrLocator")}
assert searched["InstalledSkwirdw"][1:] == ["", place("INSTALLDIR").rstrip("\\"), "0"], searched
signatures = {row[0]: row for row in table("Signature")}
assert signatures["InstalledSkwirdw"][1] == "skwirdw.exe" and not any(signatures["InstalledSkwirdw"][2:]), signatures
at = [steps[a][1] for a in ("FindRelatedProducts", "AppSearch", "CostFinalize", "SkwirdStop", "SkwirdStopOlder",
                            "InstallValidate", "RemoveExistingProducts", "InstallInitialize", "SkwirdLogonRemove",
                            "ProcessComponents", "RemoveFiles")]
assert at == sorted(at) and len(set(at)) == len(at), steps
print(f"ok: as it removes skwir, the installer runs skwirdw.exe stop before InstallValidate (step {at[3]}), then"
      f" logon remove --if-mine in its script (step {at[8]}); an upgrade removes the older first (step {at[6]})")
print(f"ok: as it upgrades a skwir after 0.7.1, it first runs the skwirdw.exe found in {place('INSTALLDIR')}"
      f" (step {at[1]}) with stop, before InstallValidate (step {at[4]})")
EOF

# The installer, per user, then removed, four times.
user="$(wine cmd /c 'echo %USERNAME%' 2>/dev/null | tr -d '\r')"
home="$WINEPREFIX/drive_c/users/$user"
installed="$home/AppData/Local/Programs/skwir"
shortcut="$home/AppData/Roaming/Microsoft/Windows/Start Menu/Programs/skwir.lnk"
# skwird's own folder, the user's (its state, cache and log), and where
# Wine keeps a logon task (system32\tasks, its actions left out: Wine
# never runs it).
skwirds="$home/AppData/Local/skwird"
tasks="$WINEPREFIX/drive_c/windows/system32/tasks"
# Installed, logged (what it found) and the programs it starts traced:
# nothing is upgraded here, so it runs none of skwird's.
installs() {
    log="$scratch/installed.log"
    trace="$scratch/installed.trace"
    WINEDEBUG=trace+process wine msiexec /i "$made/$name.msi" /qn /l*v "$(winepath -w "$log")" 2>"$trace"
    [ -f "$installed/skwir.exe" ] && [ -f "$installed/licenses/THIRD-PARTY-NOTICES" ] \
        && [ -f "$installed/share/mime/globs2" ] || { echo "the installer put nothing in $installed" >&2; exit 1; }
    grep -a -q -F "Property(S): ProductCode = " "$log" || { echo "the install's log lists no property" >&2; exit 1; }
    ! started skwirdw.exe || { echo "installed, upgrading nothing, and skwirdw.exe ran" >&2; exit 1; }
}
# Whether the install found a skwirdw.exe in the folder it puts skwir in.
found() {
    grep -a -F "Property(S): INSTALLED_SKWIRDW = " "$scratch/installed.log" |
        grep -a -q -F '\Programs\skwir\skwirdw.exe'
}
# Removed, the programs it starts traced (Wine's process channel):
# skwirdw.exe run with each action's command, and what skwird asked of
# schtasks.
removes() {
    trace="$scratch/removed.trace"
    WINEDEBUG=trace+process wine msiexec /x "$made/$name.msi" /qn 2>"$trace"
    left="$(find "$installed" -type f 2>/dev/null | wc -l)"
    [ "$left" -eq 0 ] && [ ! -e "$shortcut" ] || { echo "removed, $left files and the entry left" >&2; exit 1; }
    for command in stop 'logon remove --if-mine'; do
        started "skwirdw.exe $command\"" || { echo "removed, and skwirdw.exe $command did not run" >&2; exit 1; }
    done
}
started() { grep -a "CreateProcessInternalW" "$trace" | grep -a -q -F "$1"; }
has_task() { ls "$tasks"/skwird-S-1-* >/dev/null 2>&1; }
# skwird's logon task registered from the copy in $1, as its user would,
# or taken back by it ($2: add or remove), asked until it is there or
# gone: in a new prefix, the first call starts Wine's task service, and
# may be early for it.
logon() {
    for _ in 1 2 3 4 5; do
        said="$(wine "$1/skwird.exe" logon "$2" 2>&1)" || true
        case "$2" in
        add) if has_task; then return 0; fi ;;
        remove) if ! has_task; then return 0; fi ;;
        esac
        sleep 2
    done
    echo "skwird.exe logon $2 failed under Wine: $said" >&2
    exit 1
}
# What skwird asks schtasks to take a task back with.
deletes='/Delete /TN skwird-S-1-'
# The user's engine, run from the copy in $1 as its logon task runs it,
# on a folder of the check's own (never the prefix's home, whose folders
# Wine links to this machine's user's), until it answers; its process
# in $engine.
fixture="$scratch/indexed d'été"
mkdir -p "$fixture"
: > "$fixture/café note.txt"
serves() {
    wine "$1/skwirdw.exe" serve --root "$(winepath -w "$fixture")" --idle 600 >/dev/null 2>&1 </dev/null &
    engine=$!
    for _ in 1 2 3 4 5 6 7 8 9 10; do
        if answers "$1"; then return 0; fi
        sleep 1
    done
    echo "no engine of skwird's answers, started from $1" >&2
    exit 1
}
# Whether the user's engine answers, asked by the copy in $1, which starts
# none.
answers() { SKWIRD_NO_START=1 wine "$1/skwird.exe" status </dev/null >/dev/null 2>&1; }
runs() { kill -0 "$engine" 2>/dev/null; }
# Whether its process ended, 5 s at most after its engine said it had.
ended() {
    for _ in 1 2 3 4 5 6 7 8 9 10; do
        if ! runs; then return 0; fi
        sleep 0.5
    done
    return 1
}

# 1. Nothing of skwird's registered or running, and no folder of its own
# yet: the installer's actions, run unseen, leave none (skwirdw.exe makes
# its folder only to serve).
logon "$scratch/skwird" remove
rm -rf "${skwirds:?}"
installs
echo "ok: installed in %LocalAppData%\\Programs\\skwir, $(find "$installed" -type f | wc -l) files"
! found || { echo "installed, and a skwirdw.exe was found where none was" >&2; exit 1; }
echo "ok: no skwirdw.exe found where none was before it installed, none run"
# How the copy came (ADR-142): the installer's word, not the zip's.
[ "$(cat "$installed/share/skwir/channel")" = windows-installer ] \
    || { echo "the installed copy's channel is not windows-installer" >&2; exit 1; }
echo "ok: the installed copy's channel says windows-installer"
[ -f "$shortcut" ] || { echo "no Start menu entry: $shortcut" >&2; exit 1; }
echo "ok: a Start menu entry"
answer "$installed"
removes
[ ! -e "$skwirds" ] || { echo "removed, and skwird's actions made its folder: $skwirds" >&2; exit 1; }
echo "ok: removed, nothing of it left; skwirdw.exe ran stop and logon remove --if-mine, and made no folder of skwird's"

# 2. A skwirdw.exe already where skwir goes (left there, no skwir
# installed): found, and not run, nothing being upgraded. Then the engine
# of the installed copy, which a search would have started: stopped before
# its files go.
mkdir -p "$installed"
cp "$scratch/skwird/skwirdw.exe" "$installed/"
installs
found || { echo "installed, and the skwirdw.exe already where it installs was not found" >&2; exit 1; }
echo "ok: the skwirdw.exe already where it installs found, and not run, nothing being upgraded"
serves "$installed"
removes
ended || { echo "removed, and the installed copy's engine still runs" >&2; exit 1; }
! answers "$scratch/skwird" || { echo "removed, and an engine still answers" >&2; exit 1; }
echo "ok: removed, nothing of it left, the engine the installed copy ran stopped first"

# 3. The logon task and the engine of another copy of skwird (one
# installed on its own, its programs in a folder of their own): removing
# skwir leaves both, and skwird asks for no task to be deleted.
another="$scratch/another skwird"
mkdir -p "$another"
cp "$scratch/skwird/skwird.exe" "$scratch/skwird/skwirdw.exe" "$another/"
installs
logon "$another" add
serves "$another"
removes
! started "$deletes" || { echo "removed, and skwird asked for the task another copy registered to be deleted" >&2; exit 1; }
has_task || { echo "removed, and the logon task another copy of skwird registered was taken back" >&2; exit 1; }
if ! runs || ! answers "$another"; then
    echo "removed, and another copy's engine was stopped" >&2
    exit 1
fi
echo "ok: removed, the logon task and the engine of another copy of skwird left"
wine "$another/skwird.exe" stop >/dev/null 2>&1 || { echo "another copy's skwird.exe stop failed" >&2; exit 1; }
ended || { echo "another copy's skwird.exe stop left its engine running" >&2; exit 1; }
logon "$another" remove

# 4. The logon task the installed copy registered: skwird asks for it to
# be deleted, and it is gone.
installs
logon "$installed" add
removes
started "$deletes" || { echo "removed, and skwird did not ask for its logon task to be deleted" >&2; exit 1; }
if has_task; then
    echo "removed, skwird's logon task left: it would start nothing at each logon" >&2
    exit 1
fi
echo "ok: removed, nothing of it left, the logon task the installed copy registered taken back"

# 5. Upgrades (SkwirdStopOlder), between small installers made here from
# skwir.wxs by windows/msi.sh, as windows.sh makes the real one: skwird's
# two programs, the Start menu entry and the search. The older ones run
# nothing of skwird's as they are removed (SkwirdStop's and
# SkwirdLogonRemove's rows left out; 0.7.0's and 0.7.1's had no stop), so
# what ends an engine there is the newer's action. Their versions are made
# up, but 0.7.1, the last whose skwird has no `stop`, and below 128:
# under Wine, 200.0.0 was not after 0.7.1 (its first number read as a
# signed byte, it seems).
small="$scratch/small"
mkdir -p "$small/folder"
cp "$scratch/skwird/skwird.exe" "$scratch/skwird/skwirdw.exe" "$small/folder/"
wrestool -x -t 14 "$unpacked/$name/skwir.exe" >"$small/skwir.ico"
cat >"$small/files.wxs" <<'EOF'
<?xml version="1.0" encoding="utf-8"?>
<Wix xmlns="http://schemas.microsoft.com/wix/2006/wi">
  <Fragment>
    <DirectoryRef Id="INSTALLDIR">
      <Component Id="SkwirdConsole" Guid="*" Win64="$(var.Win64)">
        <File Id="skwird.exe" Source="$(var.Dist)/skwird.exe" KeyPath="yes" />
      </Component>
    </DirectoryRef>
    <ComponentGroup Id="Files"><ComponentRef Id="SkwirdConsole" /></ComponentGroup>
  </Fragment>
</Wix>
EOF
grep -v -e '<Custom Action="SkwirdStop" ' -e '<Custom Action="SkwirdLogonRemove" ' \
    tools/package/windows/skwir.wxs >"$small/older.wxs"
if [ "$(grep -c '<Custom Action="Skwird' tools/package/windows/skwir.wxs)" -ne 3 ] ||
    [ "$(grep -c '<Custom Action="Skwird' "$small/older.wxs")" -ne 1 ]; then
    echo "skwir.wxs's rows for skwird's actions are not those the older installers leave out" >&2
    exit 1
fi
for older in 99.0.0 0.7.1; do
    tools/package/windows/msi.sh "$older" "$small/folder" "$small/skwir.ico" "$small/files.wxs" \
        "$small/$older.msi" "$small/older.wxs"
done
tools/package/windows/msi.sh 99.0.1 "$small/folder" "$small/skwir.ico" "$small/files.wxs" "$small/99.0.1.msi"
# The older installer $1, installed; then the newer, 99.0.1, over it,
# logged and traced.
installs_older() {
    wine msiexec /i "$small/$1.msi" /qn
    [ -f "$installed/skwirdw.exe" ] || { echo "the small installer $1 put nothing in $installed" >&2; exit 1; }
}
upgraded() {
    log="$scratch/upgraded.log"
    trace="$scratch/upgraded.trace"
    WINEDEBUG=trace+process wine msiexec /i "$small/99.0.1.msi" /qn /l*v "$(winepath -w "$log")" 2>"$trace"
    has WIX_UPGRADE_DETECTED || { echo "upgrading, the older installer was not found" >&2; exit 1; }
    [ -f "$installed/skwirdw.exe" ] || { echo "upgraded, and no skwirdw.exe in $installed" >&2; exit 1; }
}
# Whether the upgrade found property $1; the line its log first names
# action $1 on, as Wine's log says it in any language (`…: Action.`).
has() { grep -a -q -F "Property(S): $1 = " "$scratch/upgraded.log"; }
step() { grep -a -n -F ": $1." "$scratch/upgraded.log" | head -1 | cut -d: -f1; }
# Whether the upgrade ran a skwirdw.exe with `stop`.
ran_stop() { grep -a CreateProcessInternalW "$trace" | grep -a -F skwirdw.exe | grep -a -q -F ' stop"'; }
# The newer removed, nothing of it left.
removes_newer() {
    wine msiexec /x "$small/99.0.1.msi" /qn
    left="$(find "$installed" -type f 2>/dev/null | wc -l)"
    [ "$left" -eq 0 ] && [ ! -e "$shortcut" ] || { echo "the newer removed, $left files and the entry left" >&2; exit 1; }
}

# From a skwir after 0.7.1 whose engine runs: the newer runs the older
# copy's skwirdw.exe stop before its own InstallValidate, and the engine
# ends.
installs_older 99.0.0
serves "$installed"
upgraded
if ! has OLDER_SKWIRD_STOPS || ! has INSTALLED_SKWIRDW; then
    echo "upgrading a skwir after 0.7.1, its skwirdw.exe or its version was not found" >&2
    exit 1
fi
stopped="$(step SkwirdStopOlder)" validated="$(step InstallValidate)"
[ -n "$stopped" ] && [ -n "$validated" ] && [ "$stopped" -lt "$validated" ] \
    || { echo "upgrading, SkwirdStopOlder did not run before InstallValidate (log lines ${stopped:-none}, ${validated:-none})" >&2; exit 1; }
ran_stop || { echo "upgrading, the older copy's skwirdw.exe stop did not run" >&2; exit 1; }
ended || { echo "upgraded, and the older copy's engine still runs" >&2; exit 1; }
! answers "$scratch/skwird" || { echo "upgraded, and an engine still answers" >&2; exit 1; }
echo "ok: upgrading a skwir after 0.7.1, the older copy's skwirdw.exe stop ran before InstallValidate, its engine ended"
removes_newer

# From 0.7.1: its skwirdw.exe found, and not run.
installs_older 0.7.1
upgraded
if ! has INSTALLED_SKWIRDW || has OLDER_SKWIRD_STOPS; then
    echo "upgrading 0.7.1, its skwirdw.exe not found or its version taken for one after it" >&2
    exit 1
fi
if [ -n "$(step SkwirdStopOlder)" ] || ran_stop; then
    echo "upgrading 0.7.1, whose skwird has no stop, its skwirdw.exe was run" >&2
    exit 1
fi
echo "ok: upgrading 0.7.1, its skwirdw.exe found and not run"
removes_newer

# From a skwir after 0.7.1 whose skwirdw.exe was deleted by hand: none
# found, none run, the upgrade done.
installs_older 99.0.0
rm -f "$installed/skwirdw.exe"
upgraded
if ! has OLDER_SKWIRD_STOPS || has INSTALLED_SKWIRDW; then
    echo "upgrading, a skwirdw.exe deleted was found, or the older's version was not" >&2
    exit 1
fi
if [ -n "$(step SkwirdStopOlder)" ] || ran_stop; then
    echo "upgrading a copy whose skwirdw.exe was deleted, a skwirdw.exe was run" >&2
    exit 1
fi
echo "ok: upgrading a copy whose skwirdw.exe was deleted, none found, none run, the upgrade done"
removes_newer
rm -f "$unfinished"
echo "$name: passed under Wine"
# What passed, where a release collects it (SKWIR_PACKAGE_OUT, as try.sh).
if [ -n "${SKWIR_PACKAGE_OUT:-}" ]; then
    for artifact in "$name.zip" "$name.msi"; do
        cp "$made/$artifact" "$made/$artifact.sha256" "$SKWIR_PACKAGE_OUT/"
    done
fi
