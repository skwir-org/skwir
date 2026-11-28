#!/usr/bin/env python3
"""Check Skwir's macOS reveal integration on a real Mac, with the real AppKit calls.

Each case asks the system to reveal a file or a folder the way another program
does (NSWorkspace's activateFileViewerSelectingURLs: and
selectFile:inFileViewerRootedAtPath:, /usr/bin/open -R, open -a), then reads what
the window shows through `skwir cmd control.status` and `skwir cmd entries`: the
folder shown, whether it is fully read, and the names selected. A case passes
only when all match. Titles and modification times are never asserted.

The app is copied into a fixture; the user's Skwir, its settings, state and
windows are never touched, and the run stops before Skwir starts unless that
holds:

- the fixture lies outside the temporary folders (default
  ~/Library/Caches/skwir-reveal-check/<token>): LaunchServices marks a bundle
  registered there `in-temp-dir` and never finds it by its identifier, so a
  reveal's sender logs "Finder replacement <id> not found" and nothing opens;
- the copy gets its own bundle identifier and LSEnvironment HOME and TMPDIR
  (the fixture's), is re-signed ad hoc and registered, and must be the one
  application LaunchServices and NSWorkspace find by that identifier before
  NSFileViewer names it. Its declared claims are the candidate's, so a claim
  that would take regular files is seen;
- a caller's HOME and TMPDIR win over LSEnvironment, in a program a launch
  request starts (open -a, and the reveal calls alike), so every sender runs
  with a bare environment (PATH, the user's name and language, nothing that
  names a folder), and before Skwir starts, a canary (the probe built as a
  bundle with the copy's declarations and LSEnvironment) proves, for each
  sender, that the system starts it with the fixture's HOME and TMPDIR and
  hands it the paths sent. Any other environment stops the run;
- every process of the copy is read (its own environment, KERN_PROCARGS2)
  once it listens and after each case: any other HOME or TMPDIR, or an
  XDG_RUNTIME_DIR, stops it and the run;
- the fixture settings turn on `plugins.control` (Agents and scripts);
- the NSFileViewer key of the global domain names the canary, then the copy,
  and is put back exactly afterwards (an absent key stays absent); nothing
  else of the global domain is written;
- only processes whose executable lies inside the fixture are stopped; the
  fixture's bundles are unregistered and the fixture removed, with what the
  system keeps per bundle identifier (preferences, saved window state) for the
  run's two identifiers alone.

Usage:
    tools/package/macos-reveal-check.py --app target/package/macos-native/skwir.app
    tools/package/macos-reveal-check.py --app PATH --case cold --timeout 60
    tools/package/macos-reveal-check.py --list

Exit: 0 every case passes; 1 a case failed; 2 setup failed, the run was not
isolated, or bad usage; 3 not macOS (skipped, nothing checked).
"""

from __future__ import annotations

import argparse
import ctypes
import fcntl
import json
import os
import plistlib
import pwd
import shutil
import signal
import struct
import subprocess
import sys
import time
import unicodedata
import uuid
from contextlib import suppress
from dataclasses import dataclass
from pathlib import Path

PROBE_SOURCE = Path(__file__).resolve().parent / "macos-reveal-probe.m"
LSREGISTER = Path(
    "/System/Library/Frameworks/CoreServices.framework/Frameworks/"
    "LaunchServices.framework/Support/lsregister"
)
SANDBOX_PROFILE = "(version 1)(allow default)"
VIEWER_KEY = "NSFileViewer"
EXIT_FAILED, EXIT_SETUP, EXIT_SKIPPED = 1, 2, 3
# The user's own home, from the user database: HOME may name anything.
REAL_HOME = Path(pwd.getpwuid(os.getuid()).pw_dir)
DEFAULT_WORK = REAL_HOME / "Library" / "Caches" / "skwir-reveal-check"
# Folders LaunchServices takes for temporary: a bundle there is never found by its id.
TEMPORARY = ("/tmp/", "/private/tmp/", "/var/folders/", "/private/var/folders/")
# What a sender keeps of the caller's environment: nothing that names a folder.
LAUNCH_KEEP = ("PATH", "USER", "LOGNAME", "LANG", "LC_ALL", "LC_CTYPE", "__CF_USER_TEXT_ENCODING")
SOCKET_MAX = 103  # sockaddr_un's sun_path, its terminating NUL left out

# Synthetic names with the properties that matter: spaces, non-ASCII in composed
# (NFC) and decomposed (NFD) form, a package.
CAFE = "Caf\u00e9 notes.txt"
NAIVE = unicodedata.normalize("NFD", "na\u00efve file.txt")
WORDS = "two words.md"
FOLDER = unicodedata.normalize("NFD", "\u00dcn\u00efc\u00f6d\u00e9 folder")
PACKAGE = "Reveal Check Tool.app"


class Failed(Exception):
    """A case's assertion: what the window should show, and what it shows."""


class Setup(Exception):
    """The check cannot run here, or not isolated; the run stops."""


@dataclass(frozen=True)
class Case:
    name: str
    start: str  # "warm": Skwir runs already; "cold": the sender starts it; "-": no window
    sender: str  # activate | select | open-R | open-a | sandbox | defaults
    sent: tuple  # names in the fixture's files folder
    shows: str  # the folder the window must show, relative to the files folder
    selected: tuple  # the names it must select there


CASES = (
    Case("warm file, activateFileViewerSelectingURLs:", "warm", "activate", (CAFE,), ".", (CAFE,)),
    Case("cold file, activateFileViewerSelectingURLs:", "cold", "activate", (CAFE,), ".", (CAFE,)),
    Case("warm file with NFD name, selectFile:inFileViewerRootedAtPath:", "warm", "select", (NAIVE,), ".", (NAIVE,)),
    Case("warm file, /usr/bin/open -R", "warm", "open-R", (WORDS,), ".", (WORDS,)),
    Case("warm three files of one folder, activate", "warm", "activate", (CAFE, NAIVE, WORDS), ".", (CAFE, NAIVE, WORDS)),
    Case("warm folder by URL, activate (opens into the folder)", "warm", "activate", (FOLDER,), FOLDER, ()),
    Case("warm folder, open -a (odoc: same as a reveal)", "warm", "open-a", (FOLDER,), FOLDER, ()),
    Case("warm .app package selected, not entered", "warm", "activate", (PACKAGE,), ".", (PACKAGE,)),
    Case("cold .app package selected, not entered", "cold", "activate", (PACKAGE,), ".", (PACKAGE,)),
    Case("warm file, sandboxed sender (sandbox-exec allow default)", "warm", "sandbox", (WORDS,), ".", (WORDS,)),
    Case("regular files not claimed by the candidate", "-", "defaults", (CAFE, WORDS), "", ()),
)

# What the canary is sent to prove each sender's route ("start": open -a, no paths).
ROUTE_PROOFS = {
    "start": (),
    "activate": (CAFE,),
    "select": (NAIVE,),
    "open-R": (WORDS,),
    "open-a": (FOLDER,),
    "sandbox": (WORDS,),
}


def run(cmd, **kwargs) -> subprocess.CompletedProcess:
    return subprocess.run(cmd, capture_output=True, text=True, **kwargs)


def must(cmd, what, **kwargs) -> subprocess.CompletedProcess:
    done = run(cmd, **kwargs)
    if done.returncode != 0:
        raise Setup(f"{what} failed: {(done.stderr or done.stdout).strip()}")
    return done


def real(path) -> str:
    return os.path.realpath(path)


def inside(path: str, folder) -> bool:
    return real(path).startswith(real(folder) + "/")


# Processes, read from the kernel: each one's program and its own environment.

_system = ctypes.CDLL("/usr/lib/libSystem.B.dylib", use_errno=True) if sys.platform == "darwin" else None
CTL_KERN, KERN_ARGMAX, KERN_PROCARGS2 = 1, 8, 49


def all_pids() -> list[int]:
    count = _system.proc_listallpids(None, 0)
    buffer = (ctypes.c_int * (count + 256))()
    count = _system.proc_listallpids(buffer, ctypes.sizeof(buffer))
    return [pid for pid in buffer[: max(count, 0)] if pid > 0]


def program_of(pid: int) -> str | None:
    buffer = ctypes.create_string_buffer(4096)
    if _system.proc_pidpath(pid, buffer, ctypes.sizeof(buffer)) <= 0:
        return None
    return os.fsdecode(buffer.value)


def _sysctl(mib: tuple, size: int) -> bytes | None:
    name = (ctypes.c_int * len(mib))(*mib)
    buffer = ctypes.create_string_buffer(size)
    length = ctypes.c_size_t(size)
    if _system.sysctl(name, len(mib), buffer, ctypes.byref(length), None, ctypes.c_size_t(0)) != 0:
        return None
    return buffer.raw[: length.value]


def environment_of(pid: int, names: tuple) -> dict[str, str | None] | None:
    """The named variables of a process's own environment; None when it cannot be read."""
    argmax = _sysctl((CTL_KERN, KERN_ARGMAX), ctypes.sizeof(ctypes.c_int))
    raw = argmax and _sysctl((CTL_KERN, KERN_PROCARGS2, pid), struct.unpack("i", argmax)[0])
    if not raw:
        return None
    argc = struct.unpack_from("i", raw)[0]
    position = raw.index(b"\0", 4)  # the program's path
    while position < len(raw) and raw[position] == 0:
        position += 1
    for _ in range(argc):
        position = raw.index(b"\0", position) + 1
    wanted = {name.encode(): name for name in names}
    found: dict[str, str | None] = dict.fromkeys(names)
    variables = 0
    while position < len(raw):
        end = raw.find(b"\0", position)
        if end <= position:
            break
        key, _, value = raw[position:end].partition(b"=")
        if key in wanted:
            found[wanted[key]] = os.fsdecode(value)
        variables += 1
        position = end + 1
    # The system hides a platform program's environment: no variable at all is not an answer.
    return found if variables else None


def running_in(folder: Path) -> list[int]:
    """Processes whose program lies inside `folder`: the only ones ever stopped."""
    return [pid for pid in all_pids() if (program := program_of(pid)) and inside(program, folder)]


def stop(folder: Path) -> None:
    for sig, grace in ((signal.SIGTERM, 10.0), (signal.SIGKILL, 5.0)):
        if not running_in(folder):
            return
        for pid in running_in(folder):  # read again just before: a pid is never reused blind
            with suppress(ProcessLookupError):
                os.kill(pid, sig)
        deadline = time.monotonic() + grace
        while running_in(folder) and time.monotonic() < deadline:
            time.sleep(0.2)
    if running_in(folder):
        raise Setup(f"the processes of {folder} did not stop")


def app_executable(app: Path) -> str:
    if app.suffix != ".app" or not app.is_dir():
        raise Setup(f"--app must be a .app bundle: {app}")
    info = app / "Contents" / "Info.plist"
    if not info.is_file():
        raise Setup(f"no Contents/Info.plist in {app}")
    name = plistlib.loads(info.read_bytes()).get("CFBundleExecutable")
    if not name or not (app / "Contents" / "MacOS" / name).is_file():
        raise Setup(f"no CFBundleExecutable in {app}")
    return name


def launch_env() -> dict:
    """The environment of every sender and launch request: a caller's HOME or TMPDIR
    would win over the copy's LSEnvironment, so none is passed (proved on the canary)."""
    env = {k: os.environ[k] for k in LAUNCH_KEEP if k in os.environ}
    env.setdefault("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
    return env


class Fixture:
    """The copy under test, the canary, and everything the run makes beside them."""

    def __init__(self, root: Path, token: str):
        self.root = root
        self.bundle = root / "app" / "skwir.app"
        self.bundle_id = f"org.skwir.skwir.revealcheck.{token}"
        self.canary = root / "canary" / "canary.app"
        self.canary_id = f"{self.bundle_id}.canary"
        self.canary_log = root / "canary" / "canary.jsonl"
        self.home = root / "home"
        self.runtime = root / "t"  # TMPDIR: the control socket is runtime/skwir/control.sock
        self.files = root / "files"
        self.probe = root / "probe"
        self.binary = self.bundle  # set by build
        self.env = {"HOME": str(self.home), "TMPDIR": str(self.runtime), "PATH": launch_env()["PATH"]}
        self.isolated = {"HOME": str(self.home), "TMPDIR": str(self.runtime), "XDG_RUNTIME_DIR": None}
        self.registered: list[Path] = []
        self.owns_residues = False

    def residues(self) -> list[Path]:
        """What the system keeps per bundle identifier, in the user's own Library, for this run's two."""
        library = REAL_HOME / "Library"
        return [
            path
            for bundle_id in (self.bundle_id, self.canary_id)
            for path in (
                library / "Preferences" / f"{bundle_id}.plist",
                library / "Saved Application State" / f"{bundle_id}.savedState",
                library / "Caches" / bundle_id,
                library / "HTTPStorages" / bundle_id,
                library / "HTTPStorages" / f"{bundle_id}.binarycookies",
                library / "WebKit" / bundle_id,
            )
        ]

    def build(self, source: Path, executable: str) -> None:
        if len(os.fsencode(self.runtime / "skwir" / "control.sock")) > SOCKET_MAX:
            raise Setup(f"the fixture's control socket path is too long; use a shorter --work than {self.root.parent}")
        if any(path.exists() for path in self.residues()):
            raise Setup("the system already keeps something for this run's bundle identifier")
        self.owns_residues = True
        self.binary = self.bundle / "Contents" / "MacOS" / executable
        settings = self.home / "Library" / "Application Support" / "skwir"
        for folder in (settings, self.runtime, self.files, self.canary.parent):
            folder.mkdir(parents=True)
        (settings / "settings.toml").write_text("[plugins.control]\nenabled = true\n")
        environment = {"HOME": str(self.home), "TMPDIR": str(self.runtime)}

        shutil.copytree(source, self.bundle, symlinks=True)
        info_path = self.bundle / "Contents" / "Info.plist"
        info = plistlib.loads(info_path.read_bytes())
        candidate = dict(info)
        info["CFBundleIdentifier"] = self.bundle_id
        info["LSEnvironment"] = environment
        info_path.write_bytes(plistlib.dumps(info))
        must(["xattr", "-cr", str(self.bundle)], "clearing quarantine on the copy")
        must(["codesign", "--force", "--deep", "--sign", "-", str(self.bundle)], "re-signing the copy")

        must(
            ["clang", "-fobjc-arc", "-Wall", "-framework", "AppKit", "-framework", "CoreServices",
             "-framework", "Foundation", "-o", str(self.probe), str(PROBE_SOURCE)],
            "building the native sender",
        )
        # The canary: the probe in a bundle with the candidate's declarations.
        (self.canary / "Contents" / "MacOS").mkdir(parents=True)
        shutil.copy2(self.probe, self.canary / "Contents" / "MacOS" / "probe")
        canary = {k: v for k, v in candidate.items() if k not in ("CFBundleIconFile", "CFBundleDisplayName")}
        canary.update(CFBundleIdentifier=self.canary_id, CFBundleExecutable="probe",
                      CFBundleName="revealcheck", LSEnvironment=environment)
        (self.canary / "Contents" / "Info.plist").write_bytes(plistlib.dumps(canary))
        must(["codesign", "--force", "--sign", "-", str(self.canary)], "signing the canary")

        for bundle, bundle_id in ((self.bundle, self.bundle_id), (self.canary, self.canary_id)):
            self.registered.append(bundle)
            must([str(LSREGISTER), "-f", str(bundle)], f"registering {bundle}")
            self.found(bundle, bundle_id)

        for name in (CAFE, NAIVE, WORDS):
            (self.files / name).write_text("fixture\n")
        (self.files / FOLDER).mkdir()
        (self.files / FOLDER / "inside.txt").write_text("fixture\n")
        package = self.files / PACKAGE / "Contents"
        package.mkdir(parents=True)
        (package / "Info.plist").write_bytes(
            plistlib.dumps({"CFBundlePackageType": "APPL", "CFBundleName": "Reveal Check Tool"})
        )

    def found(self, bundle: Path, bundle_id: str, timeout: float = 15.0) -> None:
        """The system finds `bundle`, and it alone, by `bundle_id`, as a reveal's sender looks it up."""
        deadline = time.monotonic() + timeout
        while True:
            done = must([str(self.probe), "lookup", bundle_id], "the identifier lookup", env=launch_env())
            answer = json.loads(done.stdout)
            listed = [real(p) for p in answer["launch_services"]]
            if listed == [real(bundle)] and answer["workspace"] and real(answer["workspace"]) == real(bundle):
                print(f"setup: {bundle_id} is found at {bundle} by LaunchServices and NSWorkspace")
                return
            if len(listed) > 1 or time.monotonic() >= deadline:
                raise Setup(
                    f"{bundle_id} is registered from {bundle}, but the system finds {listed or 'nothing'} "
                    f"(error {answer['launch_services_error']}) and NSWorkspace {answer['workspace']}; "
                    "a reveal would not reach it"
                )
            time.sleep(0.5)

    def unisolated(self, env: dict | None) -> str | None:
        """What is wrong with a process's environment, or None when it is the fixture's alone."""
        if env is None:
            return "its environment cannot be read"
        wrong = [f"{k}={env.get(k)}" for k, want in self.isolated.items() if env.get(k) != want]
        return ", ".join(wrong) or None


def cli(fx: Fixture, name: str, args: dict | None = None):
    """`skwir cmd` of the fixture's copy, its answer as JSON; None when it failed."""
    cmd = [str(fx.binary), "cmd", name]
    if args is not None:
        cmd.append(json.dumps(args))
    done = run(cmd, env=fx.env, timeout=60)
    if done.returncode != 0:
        return None
    try:
        return json.loads(done.stdout)
    except ValueError:
        return None


def listening(fx: Fixture) -> bool:
    status = cli(fx, "control.status")
    return isinstance(status, dict) and status.get("listening") is True


def text(value) -> str:
    """A name or path as the command answer gives it: text, or its bytes."""
    if isinstance(value, dict) and "bytes" in value:
        return bytes(value["bytes"]).decode("utf-8", "surrogateescape")
    return value


def location_path(location) -> str:
    if isinstance(location, dict) and location.get("scheme") == "path":
        return text(location.get("rest"))
    if isinstance(location, str) and location.startswith("/"):
        return location
    raise Failed(f"the window's location is not a folder path: {location!r}")


@dataclass(frozen=True)
class Shown:
    location: str
    complete: bool
    selected: tuple
    selected_count: int

    def __str__(self) -> str:
        return (
            f"{self.location} (complete: {self.complete}, selected {list(self.selected)}, "
            f"selected count {self.selected_count})"
        )


def observe(fx: Fixture) -> Shown | None:
    """The active window's folder and selected names; None while it is not listening."""
    if not listening(fx):
        return None
    answer = cli(fx, "entries", {"limit": 1000})
    if answer is None:
        return None
    if answer["total"] > len(answer["entries"]):
        raise Failed(f"the window lists {answer['total']} entries; the check reads 1000")
    selected = tuple(sorted(text(e["name"]) for e in answer["entries"] if e.get("selected")))
    return Shown(location_path(answer["location"]), answer["complete"], selected, answer["selected"])


def settled(shown: Shown | None, folder: Path, selected: tuple | None) -> bool:
    """Whether the window shows exactly `folder` and, when given, exactly `selected`, fully read."""
    return (
        shown is not None
        and shown.complete is True
        and real(shown.location) == real(folder)
        and (selected is None or (shown.selected == selected and shown.selected_count == len(selected)))
    )


def await_shown(fx: Fixture, timeout: float, folder: Path, selected: tuple | None) -> Shown:
    """Poll until the window shows exactly the expected state; return that reading.

    On timeout, raise Failed with the last reading: a stale or partial state
    never passes.
    """
    deadline = time.monotonic() + timeout
    while True:
        last = observe(fx)
        if settled(last, folder, selected):
            return last
        if time.monotonic() >= deadline:
            want = str(folder) if selected is None else f"{folder} with {list(selected)} selected"
            got = str(last) if last is not None else "no answer on its control channel"
            raise Failed(f"expected {want}, fully read; last reading: {got}")
        time.sleep(0.5)


def verify_env(fx: Fixture) -> None:
    """Every process of the copy runs with the fixture's environment alone; else stop it and the run."""
    for pid in running_in(fx.bundle):
        wrong = fx.unisolated(environment_of(pid, tuple(fx.isolated)))
        if wrong:
            with suppress(Setup):
                stop(fx.bundle)
            raise Setup(f"the copy (process {pid}) ran outside the fixture ({wrong}); stopped")


def await_listening(fx: Fixture, timeout: float) -> None:
    """Wait until the copy's window listens on the fixture's channel, then check its environment."""
    deadline = time.monotonic() + timeout
    while not listening(fx):
        verify_env(fx)
        if time.monotonic() >= deadline:
            raise Failed(
                f"the copy did not listen on its control channel within {timeout:.0f} s "
                "(Agents and scripts off in the fixture settings, or it did not start)"
            )
        time.sleep(0.5)
    verify_env(fx)


def send(fx: Fixture, sender: str, paths: list[str], bundle: Path) -> str:
    """Another program's call: the system's own reveal. Returns what the sender said on stderr."""
    if sender == "start":
        cmd = ["/usr/bin/open", "-a", str(bundle)]
    elif sender == "activate":
        cmd = [str(fx.probe), "activate", *paths]
    elif sender == "select":
        cmd = [str(fx.probe), "select", paths[0], ""]
    elif sender == "open-R":
        cmd = ["/usr/bin/open", "-R", *paths]
    elif sender == "open-a":
        cmd = ["/usr/bin/open", "-a", str(bundle), *paths]
    elif sender == "sandbox":
        cmd = ["sandbox-exec", "-p", SANDBOX_PROFILE, str(fx.probe), "activate", *paths]
    else:
        raise Setup(f"no sender {sender!r}")
    done = run(cmd, env=launch_env(), timeout=60)
    if done.returncode != 0:
        raise Failed(f"the sender failed: {(done.stderr or done.stdout).strip()}")
    if sender == "select" and json.loads(done.stdout).get("ok") is not True:
        raise Failed("selectFile:inFileViewerRootedAtPath: returned NO")
    return done.stderr.strip()


def prove_routes(fx: Fixture, senders: list[str], timeout: float) -> dict[str, str | None]:
    """Send each sender's request to the canary, cold. A route is proven when the canary
    was handed the paths sent; a canary started with any other environment stops the run.
    Returns, per sender, None when proven or why not."""
    unproven: dict[str, str | None] = {}
    for sender in senders:
        stop(fx.canary)
        fx.canary_log.write_text("")
        sent = [str(fx.files / name) for name in ROUTE_PROOFS[sender]]
        try:
            said = send(fx, sender, sent, fx.canary)
        except Failed as error:
            unproven[sender] = f"the canary's sender failed: {error}"
            continue
        # The system may hand a name in another normalization than it was sent in.
        same = lambda paths: sorted(unicodedata.normalize("NFC", real(p)) for p in paths)  # noqa: E731
        want = same(sent)
        deadline = time.monotonic() + timeout
        reached, events = False, []
        while not reached and time.monotonic() < deadline:
            time.sleep(0.3)
            events = [json.loads(line) for line in fx.canary_log.read_text().splitlines() if line]
            for event in events:
                wrong = fx.unisolated(event["env"])
                if wrong:
                    stop(fx.canary)
                    raise Setup(f"{sender}: the system started the canary outside the fixture ({wrong})")
            reached = any(
                (e["event"] == "launched") if not sent
                else (e["event"] == "opened" and same(e["paths"]) == want)
                for e in events
            )
        for pid in running_in(fx.canary):
            wrong = fx.unisolated(environment_of(pid, tuple(fx.isolated)))
            if wrong:
                stop(fx.canary)
                raise Setup(f"{sender}: the canary runs outside the fixture ({wrong})")
        if reached:
            unproven[sender] = None
            print(f"route: {sender} reaches the fixture's viewer, in the fixture's environment")
        else:
            handed = [(e["event"], e["paths"]) for e in events]
            unproven[sender] = (
                f"the system did not hand {sent or 'a launch'} to a viewer with the candidate's declarations "
                f"(the canary) in {timeout:.0f} s; it had {handed}"
            ) + (
                f"; the sender said: {said}" if said else ""
            )
            print(f"route: {sender} NOT proven: {unproven[sender]}")
    stop(fx.canary)
    return unproven


def check_defaults(case: Case, fx: Fixture) -> str:
    """Regular files must still open in another application, never the candidate."""
    lines = []
    for name in case.sent:
        done = must([str(fx.probe), "default", str(fx.files / name)], "the default-application query",
                    env=launch_env())
        bundle = json.loads(done.stdout)["default_bundle_id"]
        if bundle in (fx.bundle_id, fx.canary_id):
            raise Failed(f"{name} would open in the candidate ({bundle}) by default")
        lines.append(f"{name} -> {bundle}")
    return "; ".join(lines)


def run_case(case: Case, fx: Fixture, routes: dict, timeout: float, launch_timeout: float) -> str:
    if case.sender == "defaults":
        return check_defaults(case, fx)
    if routes.get(case.sender):
        raise Failed(f"not sent, its route is not proven: {routes[case.sender]}")
    if case.start == "cold":
        stop(fx.bundle)
    else:
        if not running_in(fx.bundle):
            send(fx, "start", [], fx.bundle)
        await_listening(fx, launch_timeout)
        # A known starting folder, so a state left by an earlier case cannot pass.
        if cli(fx, "navigate", {"location": str(fx.home)}) is None:
            raise Failed("the copy did not navigate to the fixture's home before the case")
        await_shown(fx, timeout, fx.home, None)
    stderr = send(fx, case.sender, [str(fx.files / name) for name in case.sent], fx.bundle)
    try:
        if case.start == "cold":
            await_listening(fx, launch_timeout)
        shown = await_shown(fx, timeout, fx.files / case.shows, tuple(sorted(case.selected)))
    except Failed as error:
        # The sender's own words (say, a Finder replacement not found) are the
        # likeliest cause of a reveal that never reached the copy.
        raise Failed(f"{error}" + (f"; the sender said: {stderr}" if stderr else "")) from None
    finally:
        verify_env(fx)
    return f"shows {shown.location}; selected {list(shown.selected)}"


class FileViewer:
    """The NSFileViewer key of the global domain: set for the run, put back exactly."""

    def __init__(self):
        self.original: str | None = None
        self.changed = False

    @staticmethod
    def read() -> str | None:
        kind = run(["defaults", "read-type", "-g", VIEWER_KEY])
        if kind.returncode != 0:
            return None
        if kind.stdout.strip() != "Type is string":
            raise Setup(f"NSFileViewer holds a non-string value ({kind.stdout.strip()}); not changed")
        return run(["defaults", "read", "-g", VIEWER_KEY]).stdout.rstrip("\n")

    def point(self, bundle_id: str) -> None:
        if not self.changed:
            self.original = self.read()
            print(f"setup: NSFileViewer is {self.original!r} before the run; it is put back at the end",
                  file=sys.stderr)
            self.changed = True
        must(["defaults", "write", "-g", VIEWER_KEY, "-string", bundle_id], "setting NSFileViewer")

    def restore(self) -> None:
        if not self.changed:
            return
        if self.original is None:
            run(["defaults", "delete", "-g", VIEWER_KEY])
        else:
            run(["defaults", "write", "-g", VIEWER_KEY, "-string", self.original])
        now = self.read()
        if now != self.original:
            raise Setup(f"NSFileViewer is {now!r}, not {self.original!r}: restoration failed")


def cleanup(fx: Fixture, viewer: FileViewer) -> bool:
    clean = True
    steps = (
        viewer.restore,
        lambda: stop(fx.root),
        lambda: [run([str(LSREGISTER), "-u", str(b)]) for b in (*fx.registered, fx.files / PACKAGE)],
        lambda: forget_residues(fx),
        lambda: shutil.rmtree(fx.root),
    )
    for step in steps:
        try:
            step()
        except (Setup, OSError) as error:
            clean = False
            print(f"cleanup: {error}", file=sys.stderr)
    return clean


def forget_residues(fx: Fixture) -> None:
    """What the system made for this run's identifiers, absent before (`build` checks): removed."""
    if not fx.owns_residues:
        return
    for path in fx.residues():
        if path.name.endswith(".plist") and path.exists():
            run(["defaults", "delete", path.name.removesuffix(".plist")])  # cfprefsd's copy too
        if path.is_dir() and not path.is_symlink():
            shutil.rmtree(path)
        elif path.exists() or path.is_symlink():
            path.unlink()


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Check Skwir's macOS reveal integration on a real Mac.")
    parser.add_argument("--app", help="the skwir.app to check (copied; never modified)")
    parser.add_argument("--case", action="append", default=[],
                        help="run only cases whose name contains this text (repeatable)")
    parser.add_argument("--list", action="store_true", help="list the cases and exit")
    parser.add_argument("--work", default=str(DEFAULT_WORK),
                        help=f"where the fixture is made, never a temporary folder (default {DEFAULT_WORK})")
    parser.add_argument("--timeout", type=float, default=45.0,
                        help="seconds a window may take to show each case's result")
    parser.add_argument("--launch-timeout", type=float, default=90.0,
                        help="seconds the copy or the canary may take to start")
    args = parser.parse_args(argv)
    sys.stdout.reconfigure(line_buffering=True)
    cases = [c for c in CASES if not args.case or any(f.lower() in c.name.lower() for f in args.case)]
    if not cases:
        parser.error(f"no case matches {args.case}; --list shows them")
    if args.list:
        for case in cases:
            print(f"{case.start:4}  {case.sender:8}  {case.name}")
        return 0
    if sys.platform != "darwin":
        print("SKIPPED: macos-reveal-check needs a real Mac (AppKit, LaunchServices); nothing was checked.")
        return EXIT_SKIPPED
    if not args.app:
        parser.error("--app PATH is required")
    work = Path(os.path.abspath(args.work))
    if (real(work) + "/").startswith(TEMPORARY) or (str(work) + "/").startswith(TEMPORARY):
        parser.error(f"--work {work} is a temporary folder; LaunchServices never finds an app there")
    app = Path(args.app).resolve()
    if inside(str(work), app) or real(work) == real(app):
        parser.error("--work lies inside --app")
    for sig in (signal.SIGTERM, signal.SIGHUP, signal.SIGINT):
        signal.signal(sig, lambda number, _: sys.exit(128 + number))

    work.mkdir(parents=True, exist_ok=True)
    # One lock even with different --work folders. Keep its inode: unlinking
    # a lock lets a third process bypass a second process already holding it.
    lock_path = REAL_HOME / "Library" / "Caches" / "skwir-reveal-check.lock"
    lock_path.parent.mkdir(parents=True, exist_ok=True)
    lock = open(lock_path, "a")
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        print("setup failed: another reveal check is running", file=sys.stderr)
        lock.close()
        return EXIT_SETUP
    token = uuid.uuid4().hex[:12]
    root = work / token
    root.mkdir()
    fx = Fixture(root, token)
    viewer = FileViewer()
    code = EXIT_SETUP
    try:
        fx.build(app, app_executable(app))
        used = ["start", *dict.fromkeys(c.sender for c in cases if c.sender != "defaults")]
        viewer.point(fx.canary_id)
        routes = prove_routes(fx, used if any(c.start != "-" for c in cases) else [], args.launch_timeout)
        if routes.get("start"):
            raise Setup(f"the system does not start the fixture's viewer: {routes['start']}")
        viewer.point(fx.bundle_id)
        failed = 0
        for case in cases:
            try:
                print(f"PASS  {case.name}\n      {run_case(case, fx, routes, args.timeout, args.launch_timeout)}")
            except Failed as error:
                failed += 1
                print(f"FAIL  {case.name}\n      {error}")
        print(f"\n{len(cases) - failed} passed, {failed} failed of {len(cases)}")
        code = EXIT_FAILED if failed else 0
    except Setup as error:
        print(f"setup failed: {error}", file=sys.stderr)
    finally:
        if not cleanup(fx, viewer):
            code = EXIT_SETUP
        lock.close()
        with suppress(OSError):
            work.rmdir()
    return code


if __name__ == "__main__":
    sys.exit(main())
