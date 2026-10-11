#!/usr/bin/env python3
"""Stage the shared program list, then let linuxdeploy resolve ELF dependencies.

Use relative RPATHs rather than LD_LIBRARY_PATH: the latter would affect
host programs Skwir launches. Libraries loaded without a DT_NEEDED edge
(PDFium, Vulkan, GStreamer plugins) are explicit inputs.
"""

import os
import fnmatch
import json
import pathlib
import shutil
import subprocess

ROOT = pathlib.Path.cwd()
APP = pathlib.Path("/build/AppDir")
TOOLS = ROOT / "tools/package"

# These interfaces must match the host's display/audio drivers and their
# modules. libc's own libraries are discovered from its package below.
HOST_ABI = ("libEGL.so.*", "libGL.so.*", "libGLX.so.*", "libGLdispatch.so.*",
            "libOpenGL.so.*", "libdrm*.so.*", "libgbm.so.*", "libnvidia-*",
            "libvulkan_*.so*", "libasound.so.*", "libstdc++.so.*", "libgcc_s.so.*",
            "libX11.so.*", "libX11-xcb.so.*", "libxcb.so.*", "libwayland-client.so.*")


def elf(path):
    if not path.is_file() or path.is_symlink():
        return False
    with path.open("rb") as stream:
        return stream.read(4) == b"\x7fELF"


def libraries():
    """The full linked closure, except the host ABI boundary.

    linuxdeploy's generic desktop exclusions also omit optional libraries
    such as JACK and USB. Resolve from the ELFs, rather than adding an
    exception whenever a codec introduces another dependency.
    """
    libc = {pathlib.Path(p).resolve() for p in subprocess.check_output(
        ["dpkg-query", "-L", "libc6:amd64"], text=True).splitlines() if ".so" in p}
    found = set()
    for path in APP.rglob("*"):
        if not elf(path):
            continue
        linked = subprocess.check_output(["ldd", str(path)], text=True)
        assert "not found" not in linked, (path, linked)
        for line in linked.splitlines():
            name = line.split("=>")[-1].strip().rsplit(" (", 1)[0]
            if not name.startswith("/"):
                continue
            library = pathlib.Path(name).resolve()
            if library not in libc and not any(fnmatch.fnmatch(library.name, p) for p in HOST_ABI):
                found.add(library)
    return found


def copy(source, dest):
    dest.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, dest)


def main():
    if APP.exists():
        shutil.rmtree(APP)
    helpers = APP / "usr/lib/skwir"
    programs = (TOOLS / "programs").read_text().splitlines()
    for program in programs:
        dest = APP / "usr/bin" if program == "skwir" else helpers
        copy(pathlib.Path(os.environ["CARGO_TARGET_DIR"]) / "dist" / program, dest / program)
    copy(ROOT / "target/pdfium/libpdfium.so", helpers / "libpdfium.so")
    copy(TOOLS / "linux/org.skwir.skwir.desktop", APP / "usr/share/applications/org.skwir.skwir.desktop")
    copy(TOOLS / "linux/org.skwir.skwir.metainfo.xml", APP / "usr/share/metainfo/org.skwir.skwir.metainfo.xml")
    copy(ROOT / "crates/ui/assets/org.skwir.skwir.svg", APP / "usr/share/icons/hicolor/scalable/apps/org.skwir.skwir.svg")
    # This marker identifies the private, read-only runtime to sandboxes.
    copy(TOOLS / "appimage/runtime", APP / "usr/share/skwir/appimage-runtime")
    licences = APP / "usr/share/licenses/skwir"
    copy(ROOT / "LICENSE", licences / "LICENSE")
    shutil.copytree(ROOT / "target/pdfium/licenses", licences, dirs_exist_ok=True)
    shutil.copytree(ROOT / "target/notices", licences / "crates")
    shutil.copytree(TOOLS / "licenses", licences / "readers")
    for notice in pathlib.Path("/inputs").glob("runtime*"):
        if notice.name != "runtime-x86_64":
            copy(notice, licences / "appimage" / notice.name)
    # Include the distribution's copyright texts for bundled native
    # libraries and their referenced common licences (including codecs).
    for copyright_file in pathlib.Path("/usr/share/doc").glob("*/copyright"):
        copy(copyright_file, licences / "ubuntu" / copyright_file.parent.name / "copyright")
    shutil.copytree("/usr/share/common-licenses", licences / "common-licenses")
    packages = subprocess.check_output([
        "dpkg-query", "-W", "-f=${binary:Package}\t${Version}\t${source:Package}\t${source:Version}\n",
    ], text=True)
    (licences / "ubuntu-packages.tsv").write_text(packages)

    multiarch = pathlib.Path("/usr/lib/x86_64-linux-gnu")
    plugins = multiarch / "gstreamer-1.0"
    # Source-package ownership, not filenames: upstream may merge or
    # rename plugins (1.28's videoconvertscale replaces two 1.20 plugins).
    # Keeping the old ones registers duplicate types in the scanner.
    replaced = set()
    for row in packages.splitlines():
        package, _, source, _ = row.split("\t")
        if source in {"gstreamer1.0", "gst-plugins-base1.0"}:
            replaced.update(subprocess.check_output(["dpkg-query", "-L", package], text=True).splitlines())
    for plugin in plugins.glob("*.so"):
        if str(plugin) not in replaced:
            copy(plugin, helpers / "gstreamer-1.0" / plugin.name)
    runtime = pathlib.Path("/opt/skwir-gstreamer")
    shutil.copytree(runtime / "lib/gstreamer-1.0", helpers / "gstreamer-1.0", symlinks=False, dirs_exist_ok=True)
    copy(runtime / "libexec/gstreamer-1.0/gst-plugin-scanner", helpers / "gst-plugin-scanner")
    inputs = json.loads((TOOLS / "appimage/downloads.json").read_text())
    for name in inputs:
        if name.startswith(("gstreamer-", "gst-plugins-base-", "orc-")) and name.endswith(".tar.xz"):
            # Exact unmodified sources, including their copyright and
            # licence notices, travel with the native components built here.
            copy(pathlib.Path("/inputs") / name, licences / "gstreamer" / name)
    deploy = "/opt/linuxdeploy/squashfs-root/AppRun"
    linked = libraries() | {(multiarch / name).resolve() for name in ("libvulkan.so.1", "libfontconfig.so.1")}
    # Explicit --library inputs still obey linuxdeploy's exclusion list.
    # Stage our resolved closure ourselves, including each ELF's SONAME.
    for library in sorted(linked):
        copy(library, APP / "usr/lib" / library.name)
        soname = subprocess.check_output(["patchelf", "--print-soname", str(library)], text=True).strip()
        if soname and soname != library.name:
            alias = APP / "usr/lib" / soname
            if alias.is_symlink():
                assert os.readlink(alias) == library.name, (alias, library)
            else:
                alias.symlink_to(library.name)
    subprocess.run([
        deploy, "--appdir", str(APP),
        "--desktop-file", str(APP / "usr/share/applications/org.skwir.skwir.desktop"),
        "--icon-file", str(APP / "usr/share/icons/hicolor/scalable/apps/org.skwir.skwir.svg"),
    ], check=True)
    # linuxdeploy processes all ELFs already in the AppDir, including the
    # plugins and helpers. Give every ELF its own relative RUNPATH, rather
    # than an inherited RPATH that could force our older libraries on a
    # host graphics driver. The system retains its libc and drivers.
    lib = APP / "usr/lib"
    for path in APP.rglob("*"):
        if not elf(path):
            continue
        relative = os.path.relpath(lib, path.parent)
        subprocess.run(["patchelf", "--set-rpath", f"$ORIGIN:$ORIGIN/{relative}", str(path)], check=True)
    # The wrapper configures GStreamer for this child alone. Both the
    # real binary and the plugins stay under the helper's runtime prefix.
    (helpers / "skwir-media-worker").rename(helpers / "skwir-media-worker.bin")
    copy(TOOLS / "appimage/media-worker", helpers / "skwir-media-worker")


if __name__ == "__main__":
    main()
