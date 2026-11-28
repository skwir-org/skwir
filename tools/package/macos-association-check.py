#!/usr/bin/env python3
"""Observe folder associations on a disposable GitHub-hosted Mac, never a personal Mac.

Uses the already-tested reveal fixture for the real app's private environment.
OS refusals are results, not test failures. Invalid instrumentation or failed
restoration fails the job. No Rust compilation, system restart or SIP changes.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import os
from pathlib import Path
import plistlib
import signal
import subprocess
import sys
import time
import uuid

DOMAIN = "com.apple.LaunchServices/com.apple.launchservices.secure"
SOURCE = Path(__file__).resolve().parent


def run(*args, **kwargs):
    return subprocess.run(args, capture_output=True, text=True, timeout=60, **kwargs)


def checked(*args, **kwargs):
    result = run(*args, **kwargs)
    if result.returncode:
        raise RuntimeError(f"{args[0]} failed: {result.stderr or result.stdout}")
    return result.stdout


def preferences():
    result = run("defaults", "export", DOMAIN, "-")
    if result.returncode:
        if "does not exist" in result.stderr:
            return {}
        raise RuntimeError(f"cannot read LaunchServices preferences: {result.stderr}")
    return plistlib.loads(result.stdout.encode())


def attributes(path):
    return checked("xattr", str(path)).splitlines()


def read_association(path):
    return plistlib.loads(bytes.fromhex(
        checked("xattr", "-px", "com.apple.LaunchServices.OpenWith", str(path))))


def write_association(path, value):
    checked("xattr", "-wx", "com.apple.LaunchServices.OpenWith",
            plistlib.dumps(value, fmt=plistlib.FMT_BINARY).hex(), str(path))


def restore_handlers(original):
    # Preserve unrelated preferences, including concurrent LaunchServices updates.
    if "LSHandlers" in original:
        checked("defaults", "write", DOMAIN, "LSHandlers",
                plistlib.dumps(original["LSHandlers"]).decode())
    else:
        result = run("defaults", "delete", DOMAIN, "LSHandlers")
        if result.returncode and "LSHandlers" in preferences():
            raise RuntimeError("cannot remove the diagnostic's LSHandlers")
    if preferences().get("LSHandlers") != original.get("LSHandlers"):
        raise RuntimeError("LSHandlers restoration did not read back exactly")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    # This tool changes associations. A personal Mac must stop before any I/O.
    if (sys.platform != "darwin" or os.environ.get("GITHUB_ACTIONS") != "true"
            or os.environ.get("RUNNER_ENVIRONMENT") != "github-hosted"):
        parser.error("requires a disposable GitHub-hosted macOS runner; local execution refused")
    spec = importlib.util.spec_from_file_location("association_reveal", SOURCE / "macos-reveal-check.py")
    reveal = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = reveal
    spec.loader.exec_module(reveal)
    token = uuid.uuid4().hex[:10]
    root = reveal.REAL_HOME / "Library" / "Caches" / "skwir-associations" / token
    root.mkdir(parents=True)
    fx = reveal.Fixture(root, token)
    viewer = reveal.FileViewer()
    original = preferences()
    original_viewer = viewer.read()
    results = {"macos": checked("sw_vers").strip(), "cases": [], "instrumentation": {}}
    code = 2
    mounted_volume = None

    def emit(value):
        # Public CI artifacts contain synthetic paths, not user machine details.
        return json.loads(json.dumps(value).replace(str(root), "<fixture>")
                          .replace(str(reveal.REAL_HOME), "<runner-home>"))

    try:
        fx.build(args.app.resolve(), reveal.app_executable(args.app.resolve()))
        probe = root / "associations"
        checked("clang", "-fobjc-arc", "-Wall", "-framework", "AppKit",
                "-framework", "CoreServices", "-framework", "UniformTypeIdentifiers", "-o", str(probe),
                str(SOURCE / "macos-association-probe.m"))

        def native(*arguments):
            answer = checked(str(probe), *map(str, arguments), env=reveal.launch_env())
            return json.loads(answer)

        def refresh_launch_services():
            # Preference restoration alone does not invalidate lsd's in-memory
            # choice. This is a disposable CI account, never the user's session.
            pids = run("/usr/bin/pgrep", "-u", str(os.getuid()), "-x", "lsd")
            for value in pids.stdout.split():
                pid = int(value)
                if reveal.program_of(pid) != "/usr/libexec/lsd":
                    raise RuntimeError("refusing to signal an unverified LaunchServices process")
                try:
                    os.kill(pid, signal.SIGTERM)
                except ProcessLookupError:
                    pass
                deadline = time.monotonic() + 5
                while reveal.program_of(pid) == "/usr/libexec/lsd":
                    if time.monotonic() >= deadline:
                        raise RuntimeError("the owned LaunchServices process did not stop")
                    time.sleep(0.05)
            # A fresh query starts the service again. Explicit registrations
            # under Caches may be transient, so re-register both owned bundles.
            native("query", target)
            for bundle, identifier in ((fx.canary, fx.canary_id), (fx.bundle, fx.bundle_id)):
                checked(str(reveal.LSREGISTER), "-f", str(bundle))
                fx.found(bundle, identifier)

        # A known-capable folder viewer distinguishes OS policy from Skwir's
        # deliberately non-default public.item declaration.
        info_path = fx.canary / "Contents/Info.plist"
        info = plistlib.loads(info_path.read_bytes())
        info["CFBundleDocumentTypes"] = [{
            "CFBundleTypeName": "Diagnostic folder",
            "CFBundleTypeRole": "Viewer", "LSHandlerRank": "Owner",
            "LSItemContentTypes": ["public.folder", "public.directory", "public.volume"],
        }]
        info["CFBundleURLTypes"] = [{
            "CFBundleURLName": "Diagnostic file URL",
            "CFBundleURLSchemes": ["file"],
            "CFBundleTypeRole": "Viewer",
        }]
        info_path.write_bytes(plistlib.dumps(info))
        checked("codesign", "--force", "--sign", "-", str(fx.canary))
        checked(str(reveal.LSREGISTER), "-f", str(fx.canary))
        fx.found(fx.canary, fx.canary_id)
        routes = reveal.prove_routes(fx, ["open-a"], 20)
        if routes["open-a"]:
            raise RuntimeError(f"explicit canary launch is not proven: {routes['open-a']}")
        results["instrumentation"]["canary_explicit_open"] = True
        results["app_version"] = checked(str(fx.binary), "--version").strip()
        target = fx.files / "Association Target"
        control = fx.files / "Unchanged Control"
        for folder in (target, control):
            folder.mkdir()
            (folder / "association-check.txt").write_text("synthetic association fixture\n")

        # Prove actual Skwir can display this folder before using its absence
        # as evidence. The earlier canary proved the isolated launch environment.
        checked("/usr/bin/open", "-g", "-a", str(fx.bundle), str(target), env=reveal.launch_env())
        reveal.await_listening(fx, 45)
        shown = reveal.await_shown(fx, 30, target, ())
        results["instrumentation"]["skwir_explicit_open"] = {
            "location": shown.location, "complete": shown.complete}
        reveal.stop(fx.bundle)
        positive = fx.files / "Positive Control.txt"
        positive.write_text("synthetic regular file\n")
        positive_result = native("path", "/System/Applications/TextEdit.app", positive)
        positive_query = native("query", positive)
        if not positive_result.get("ok") or positive_query["bundle_id"] != "com.apple.TextEdit":
            raise RuntimeError(f"regular-file positive control failed: {positive_result}, {positive_query}")
        if read_association(positive).get("bundleidentifier") != "com.apple.TextEdit":
            raise RuntimeError("regular-file positive control lacks the expected stored association")
        results["instrumentation"]["regular_file_api"] = positive_result
        baseline = {"target": native("query", target), "control": native("query", control)}
        results["instrumentation"]["baseline"] = baseline
        # A mounted disk is a separate content type, not another folder alias.
        # Create and detach only this disposable host's owned sparse image.
        volume = root / "volume"
        volume.mkdir()
        image = root / "volume.sparseimage"
        checked("hdiutil", "create", "-size", "64m", "-fs", "APFS", "-type", "SPARSE",
                "-volname", "Skwir Association Volume", str(image))
        mounted_volume = volume
        checked("hdiutil", "attach", "-nobrowse", "-noautoopen", "-mountpoint", str(volume),
                str(image))
        resource = native("resource", volume)
        if not resource.get("is_volume") or not os.path.ismount(volume):
            raise RuntimeError(f"the volume fixture is not a mounted volume: {resource}")
        (volume / "association-check.txt").write_text("synthetic volume association fixture\n")
        volume_baseline = {"target": native("query", volume), "control": native("query", control)}
        results["instrumentation"]["volume"] = {
            "resource": resource, "baseline": volume_baseline}

        for app, bundle_id, label in (
            (fx.canary, fx.canary_id, "capable_canary"),
            (fx.bundle, fx.bundle_id, "skwir"),
        ):
            for method in ("per_path_api", "per_path_xattr", "folder_all_api",
                           "folder_viewer_api", "directory_all_api", "raw_folder_preference",
                           "modern_folder_api", "modern_folder_by_file_api", "file_scheme_api",
                           "volume_all_api"):
                item = volume if method == "volume_all_api" else target
                expected_baseline = volume_baseline if method == "volume_all_api" else baseline
                reveal.stop(fx.canary)
                reveal.stop(fx.bundle)
                for folder in (target, control):
                    if "com.apple.LaunchServices.OpenWith" in attributes(folder):
                        checked("xattr", "-d", "com.apple.LaunchServices.OpenWith", str(folder))
                restore_handlers(original)
                refresh_launch_services()
                fx.canary_log.write_text("")
                before = {"target": native("query", item), "control": native("query", control)}
                if before != expected_baseline:
                    raise RuntimeError(f"case not isolated: baseline {expected_baseline}, got {before}")
                if method == "per_path_api":
                    mutation = native("path", app, target)
                elif method == "modern_folder_api":
                    mutation = native("modern-type", app, "public.folder")
                elif method == "modern_folder_by_file_api":
                    mutation = native("modern-file-type", app, target)
                elif method == "file_scheme_api":
                    mutation = native("scheme", app, "file")
                elif method == "volume_all_api":
                    mutation = native("type", "public.volume", "all", bundle_id)
                elif method == "per_path_xattr":
                    attribute = {"version": 0, "path": str(app), "bundleidentifier": bundle_id}
                    write_association(target, attribute)
                    stored = read_association(target)
                    mutation = {"stored_exactly": stored == attribute}
                elif method == "raw_folder_preference":
                    handlers = list(original.get("LSHandlers", []))
                    handlers = [h for h in handlers if h.get("LSHandlerContentType") != "public.folder"]
                    handlers.append({"LSHandlerContentType": "public.folder", "LSHandlerRoleAll": bundle_id})
                    checked("defaults", "write", DOMAIN, "LSHandlers", plistlib.dumps(handlers).decode())
                    mutation = {"stored_exactly": preferences().get("LSHandlers") == handlers}
                    mutation["before_service_refresh"] = native("query", target)
                    refresh_launch_services()
                else:
                    content_type = "public.directory" if method == "directory_all_api" else "public.folder"
                    role = "viewer" if method == "folder_viewer_api" else "all"
                    mutation = native("type", content_type, role, bundle_id)
                after = {"target": native("query", item), "control": native("query", control)}
                # Plain command-line open, no explicit application and no reveal.
                opened = run("/usr/bin/open", "-g", str(item), env=reveal.launch_env())
                record = {
                    "application": label, "method": method, "mutation": mutation,
                    "scope": "volume" if method == "volume_all_api" else "folder",
                    "before": before, "after": after,
                    "plain_open": {"returncode": opened.returncode, "stderr": opened.stderr.strip()},
                }
                if opened.returncode:
                    raise RuntimeError(f"ordinary open failed: {record}")
                if after["target"]["bundle_id"] == bundle_id:
                    if label == "skwir":
                        reveal.await_listening(fx, 30)
                        shown = reveal.await_shown(fx, 20, item, ())
                        reveal.verify_env(fx)
                        record["actual_open"] = {
                            "recipient": bundle_id, "location": shown.location,
                            "complete": shown.complete, "selected": list(shown.selected),
                        }
                    else:
                        deadline = time.monotonic() + 15
                        events = []
                        while time.monotonic() < deadline:
                            events = [json.loads(line) for line in fx.canary_log.read_text().splitlines() if line]
                            if any(e["event"] == "opened" and e["paths"] == [str(item)] for e in events):
                                break
                            time.sleep(0.2)
                        else:
                            raise RuntimeError("effective default is canary but ordinary open did not deliver the folder")
                        for event in events:
                            if fx.unisolated(event["env"]):
                                raise RuntimeError("canary opened outside the fixture environment")
                        record["actual_open"] = {"recipient": bundle_id, "location": str(item)}
                # Independent default-opening AppKit API reports the actual
                # recipient even when Finder, for which we have no control socket.
                record["native_default_open"] = native("open", item)
                if not record["native_default_open"].get("ok"):
                    raise RuntimeError(f"default AppKit opening failed: {record}")
                if record["native_default_open"]["recipient"] != after["target"]["bundle_id"]:
                    raise RuntimeError(f"default query and actual recipient disagree: {record}")
                if method.startswith("per_path") and after["control"] != before["control"]:
                    raise RuntimeError("per-path operation changed the unmodified control folder")
                if method == "volume_all_api" and after["control"] != before["control"]:
                    raise RuntimeError("volume operation changed the unmodified ordinary folder")
                results["cases"].append(record)
                print(json.dumps(emit(record)), flush=True)
        for folder in (target, control):
            if "com.apple.LaunchServices.OpenWith" in attributes(folder):
                checked("xattr", "-d", "com.apple.LaunchServices.OpenWith", str(folder))
        restore_handlers(original)
        refresh_launch_services()
        restored = {"target": native("query", target), "control": native("query", control)}
        if restored != baseline:
            raise RuntimeError(f"effective defaults not restored: {restored}")
        restored_volume = {"target": native("query", volume), "control": native("query", control)}
        if restored_volume != volume_baseline:
            raise RuntimeError(f"effective volume default not restored: {restored_volume}")
        results["effective_defaults_restored"] = True
        code = 0
    except Exception as error:
        results["error"] = str(error)
        print(f"diagnostic failed: {error}", file=sys.stderr, flush=True)
    finally:
        try:
            restore_handlers(original)
            if viewer.read() != original_viewer:
                raise RuntimeError("NSFileViewer unexpectedly changed")
            results["restored"] = True
        except Exception as error:
            results["cleanup_error"] = str(error)
            code = 2
        detached = True
        if mounted_volume is not None and os.path.ismount(mounted_volume):
            try:
                reveal.stop(fx.root)
                checked("hdiutil", "detach", str(mounted_volume))
                if os.path.ismount(mounted_volume):
                    raise RuntimeError("the owned volume is still mounted")
                results["volume_detached"] = True
            except Exception as error:
                results["volume_cleanup_error"] = str(error)
                detached = False
                code = 2
        # Never recursively remove a fixture while its image is still mounted.
        if detached and not reveal.cleanup(fx, viewer):
            results["fixture_cleanup_failed"] = True
            code = 2
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(emit(results), indent=2) + "\n")
        print(json.dumps(emit({"completed": code == 0, "cases": len(results["cases"]),
                              "restored": results.get("restored", False)})), flush=True)
    return code


if __name__ == "__main__":
    sys.exit(main())
