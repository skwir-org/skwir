# ADR-052: A portable AppImage built in a distribution root

Status: accepted, 2026-10-11 (the user requested the implementation).
Amends ADR-040's Linux formats and Phase 5's packaging deviation.
Decision 7, the same tree as a tarball, added 2026-10-13 (the user
requested it).
Decision 2's GStreamer amended 2026-11-14 (the user's choice of the set):
all of it built at one version, not Ubuntu's 1.20 plugins beside it, and
PulseAudio's client library and libva built too.
Decision 2 amended 2026-11-22 (ADR-148): libheif and libde265 built
too, for the HEIF helper.
Decision 7 amended 2026-11-26 (ADR-050, the user's decision): the
tarball carries skwird, built in the root from its release's source;
the AppImage never does.

## Context

The retained `package-appimage` recipe wrapped the Nix package with
nix-appimage. ADR-040 recorded the concern about loading other systems'
graphics drivers with Nix's libc; no working portable artifact was
recorded. That is a limitation of the chosen payload, not the AppImage
format. A Nix host can compile against another distribution in a root
filesystem, as `tools/package/try.sh` already does.

## Decision

1. **Build on x86-64 Linux in Ubuntu 22.04**, glibc 2.35, inside a
   rootless bubblewrap namespace. No Nix store, host library paths,
   container engine or root privileges in the build. Rust follows
   `rust-toolchain.toml`. Other architectures wait for their own build
   inputs and verification, never get the x86-64 payload. A private
   binary-format registry prevents NixOS's host AppImage handler from
   intercepting the tools inside the root (Linux 6.7 or later).
2. **One artifact contains the shared program list**, the pinned PDFium,
   GStreamer's plugins and scanner, the desktop entry, metainfo, icon and
   licence notices. Resolve the complete linked dependency closure for
   linuxdeploy; its generic desktop exclusions alone omit dependencies
   of optional codecs. Dynamically loaded libraries are explicit inputs.
   Keep the host's glibc and display/audio driver interfaces (including
   ALSA). Every ELF is checked against the glibc 2.35 ceiling.
   Build pinned GStreamer 1.28.8, with Orc 0.4.44, from unmodified sources
   against this baseline: Ubuntu's 1.20 playback engine fails the current
   worker's stream selection. **Amended 2026-11-14:** every part of it at
   that version, the core, base, good, bad, ugly and gst-libav (against
   Jammy's FFmpeg 4.4), and only the plugins built here: none of Ubuntu's,
   whose stable plugin ABI kept them loading under the 1.28 core, not
   tested with it (1.20's nvvp9dec drew the keyframe for a seek while
   paused there). And PulseAudio's client library, 17.0, built from its
   sources with no daemon: Jammy's 15.99 stopped a film for good after a
   pause and a seek while paused, or a play and pause at once, then play
   (0.4.0 and 0.5.0, the picture still), against every server tried
   (PipeWire, PulseAudio 17, Jammy's own 15.99); 17.0 plays on against
   them all. The host's would leave Jammy's hosts with 15.99. Good, bad and
   ugly bring ADR-076's set, what the media helper plays with, as Windows'
   and macOS's packages have it, with Linux's decoders (VA-API, NVDEC) and
   audio output (PulseAudio; ALSA is base's); AV1 is aom's, bad's dav1d
   having moved to gst-plugins-rs and gst-libav taking no FFmpeg decoder
   that wraps another library. Every other feature is off and those named
   are required (`tools/package/appimage/gstreamer.py`): a missing library
   stops the build. Base keeps the features Ubuntu's base package has but
   its network sources (TCP's, and GIO's, which reads any scheme GIO
   knows): the helper plays `file://` alone, read by the core's `filesrc`.
   Not carried any more: GStreamer's network sources, tracker and chiptune
   music, MIDI, MXF, DV, RealMedia, and the GPL plugins whose formats FFmpeg
   decodes. libva is built too (2.24.1, the newest; its X11 and DRM
   parts, which FFmpeg's libraries link): Jammy's 2.14 opened no driver
   built for a later libva, nor any outside Debian's folder, and a newer
   one loads drivers old and new; it looks where Debian, Fedora, Arch,
   Gentoo and NixOS keep them (`-Ddriverdir`, a list; `LIBVA_DRIVERS_PATH`
   elsewhere). The host's would break FFmpeg's libraries where libva-x11
   is missing. What a library built beside GStreamer looks for where it
   runs is the host's (`/etc/pulse/client.conf`, `/etc/libva.conf`, the
   system-wide sound server's socket under `/var/run/pulse`), never the
   build's prefix, which exists only where it is built:
   `tools/package/appimage/check.py` fails on a path of that prefix in a
   staged library, but what the media helper's environment points
   elsewhere (its plugins and scanner) and what playback never reads.
   Ship the exact source archives, including their licence notices,
   beside the native library notices. **Amended 2026-11-22:** libheif
   1.23.7 and libde265 1.1.3 are built the same way, for the HEIF helper
   (ADR-148; `tools/package/appimage/heif.py`), Jammy's libheif being
   1.12: in a prefix of their own, first on the root's loader path,
   linked by the helper (`LIBRARY_PATH`), the decoders built in and AV1
   through Jammy's libaom, which GStreamer's AV1 decoder links already.
   `check.py` fails on any path of that prefix in a staged library, and
   decodes a HEIC and an AVIF with the bundled helper.
3. **Library paths stay inside the package.** Each ELF's RUNPATH is relative
   to its executable or library; it does not impose an inherited RPATH on
   host graphics drivers. AppRun exports no loader or search paths
   to the editor, terminal or converters. Only the media helper selects
   the bundled GStreamer plugins and scanner, finding them beside it, for
   itself and its scanner, and keeps their registry in Skwir's cache
   (`carried_gstreamer`, as Windows' and macOS's packages have it): the
   one a GStreamer installed on the host keeps, shared, was written again
   for one set of plugins or the other each time the other ran.
   **Amended 2026-11-14:** a wrapper script set the plugins and scanner
   before. No decoder tuning or hardware selection is added.
4. **Isolation includes the private runtime, read-only.** A sandbox for
   a bundled helper binds the package's `usr` after its private `/tmp`.
   Derive it from Skwir's executable and package marker, never from an
   outside plugin's ancestry. Other programs gain no access. No failure
   is retried unisolated because of packaging.
5. **Downloads are checksum-pinned**, including the rootfs, bundling
   tools, static AppImage runtime and its licence notices. Tool releases
   have fixed tags; a checksum mismatch fails and requires a deliberate
   lock update. The distribution's signed package updates
   remain enabled; this is a compatibility baseline, not a claim of
   byte-for-byte reproducibility.
6. **Check the artifact in a separate runtime-only root**, with no
   compiler or development libraries: ELF dependencies, helper probes,
   synthetic PDF reading/rendering and the real-window command smoke
   test on Xvfb/Mesa. The manual package workflow (ADR-053) also checks
   that same artifact on Ubuntu 24.04 and Fedora 44. Hardware Vulkan and
   Wayland need separate real-system
   checks; software rendering alone proves neither.
7. **The same tree as a tarball** (2026-10-13), for a system without
   FUSE and a folder of the user's choosing:
   `skwir-<version>-linux-x86_64.tar.gz` holds the AppImage's `usr` tree
   under one folder, with a `skwir` link to the program at its top, and
   none of the AppImage's own files. Names are sorted, owned by root and
   dated by the commit, and gzip records no name or date. It is checked
   as the AppImage is, in the same runtime-only roots, after its members
   are: one folder, no absolute or parent paths, no special files. No
   installer: the README says how to link the program onto `PATH` and
   add the desktop entry and icon.
   **Amended 2026-11-26:** the tarball carries skwird, skwir's index of
   names (ADR-050), beside skwir in `usr/bin`, where skwir looks for it
   once `PATH` has none, with its licences and its crates' notices in
   `usr/share/licenses/skwird`. It is built in the same root, against the
   same glibc, from the source archive of the release
   `tools/package/skwird.lock` pins (`tools/package/appimage/skwird.sh`):
   the archive fetched beside the root's other downloads and checked
   against its sum there and again in the root (`tools/package/skwird.py`);
   the 26 crates from crates.io its `Cargo.lock` names fetched (`cargo
   fetch --locked`, the build's one use of the network for it), then
   built offline (`--frozen`,
   `--profile dist`, as skwird's release builds it), by the root's Rust
   (skwir's channel, `RUSTUP_TOOLCHAIN`: skwird's own pin would have
   rustup download a toolchain with its checks' targets); the build's
   paths rewritten as skwird's release rewrites them (`/cargo`, `/rust`,
   `/skwird`), and the build refused if the program names a folder of the
   root. Its notices are made from skwird's own `Cargo.lock` at the tag
   (`tools/package/notices/skwird`, `notices.py`). The AppImage never
   carries skwird, nor its notices: its files are a FUSE mount that ends
   with skwir, while skwird outlives its last client, so the engine's
   program would go from under it. The tarball's tree is the AppImage's
   linked, and skwird's files are new ones beside them, never written
   into those the link shares. `check.py` refuses an AppImage holding
   any of it, and starts the tarball's on a home of its own, where it
   indexes a folder and finds a name.

## Consequences

- `just package-appimage` writes the AppImage, the tarball and their
  SHA-256 files to `target/package/appimage`. Build roots and artifacts
  stay under `target/appimage`; deleting that folder resets the build.
- Building on a Nix host is supported. Running a conventional AppImage
  on NixOS may still need `appimage-run` for its FHS runtime.
- The bundled player and alternative readers make this larger than
  the main Debian package. Their GPL/AGPL licences and native library
  notices ship with them; source remains in the release's repository.
- System tools remain optional. Bubblewrap and usable user namespaces
  are host requirements for features that require isolation, as before.
- The Debian, RPM, AUR and Nix packages remain available. The AppImage
  and the tarball are additional formats, with bundled-library updates
  our responsibility. A release carries the portable tree twice (156 MB
  as an AppImage, about 169 MB as a tarball).

## Verification

Built from the NixOS host on 2026-10-11: `just package-appimage` produced
a 156 MB image and SHA-256 file. The same image passed the separate
Ubuntu 22.04, Ubuntu 24.04 and Fedora 44 runtime roots: all 573 ELFs,
helper probes, three PDF engines, WAV playback with critical errors fatal (including
the plugin scanner), window commands, sandboxed archive extraction and
undo, and unchanged host terminal paths. The packaged media worker also
passed the synthetic H.264/AAC/subtitle tests: repeated seeks, switching
tracks while paused and playing, rate, end and restart.

On NixOS, the actual image ran through `appimage-run`; its window and
commands passed on Xvfb/lavapipe and the session's Wayland compositor
with NVIDIA graphics, using isolated files, settings and IPC. This tests
one hardware configuration; it does not establish other GPUs or actual
audio output. CI's Ubuntu 24.04/Fedora 44 checks passed locally but have
not run remotely. Phase 5, step K, in `docs/PLAN.md` records the wider
packaging status.

The tarball, 2026-10-13: `just package-appimage` made a 168.5 MB
`skwir-0.1.0-linux-x86_64.tar.gz` beside a 155.9 MB image. It passed the
same checks in the Ubuntu 22.04, Ubuntu 24.04 and Fedora 44 runtime
roots: its members first, then all 573 ELFs, helper probes, three PDF
engines, WAV playback, window commands, sandboxed archive extraction and
undo, and unchanged host terminal paths. The README's commands, run as
written in a home whose name has a space, installed it, ran it through
its link on `PATH`, and wrote a desktop entry `desktop-file-validate`
accepts.

The amendment, 2026-11-14: `just package-appimage` built PulseAudio 17.0's
client and GStreamer 1.28.8's six parts in the root and made a 195.2 MB
image (193.8 for 0.5.0's), 65 plugins (259); it passed every check in the
Ubuntu 22.04 runtime root, the film played through the bundle's
`pulsesink` among them, which 0.5.0's image failed at its seventh round.
Through `appimage-run` and the session's PipeWire, its helper played on
in 30 rounds of 30 in twelve runs over six clips, where 0.5.0's stopped
by the third (`docs/STATUS-LOG.md`, "The AppImage's films play on").

libva 2.24.1, the same day: through `appimage-run`, with NVDEC's and
FFmpeg's H.264 decoders ranked out for the test, the new image's helper
opened the Intel GPU's driver (`/run/opengl-driver/lib/dri/iHD_drv_video.so`,
`__vaDriverInit_1_24`) and decoded with `vah264dec`; 0.5.0's libva did not
open it (`va_openDriver() returns -1`).

skwird in the tarball, 2026-11-26: `tools/package/appimage/skwird.sh`
built skwird 0.2.0 in the Ubuntu 22.04 root in 53 s, and found it built
the next time (0.2 s: its source, unpacked anew, keeps the archive's
dates), its Rust the root's
1.99.1 with Linux's target alone; the program needs glibc 2.34 at most,
names `/cargo` and none of the root's folders, and links libc, libm,
libgcc_s. Then `build.sh`'s staging, without skwir's own build (its
0.7.0 programs kept from the last), made both artifacts, and `check.py`
passed each in the runtime-only root, but its `skw` check, which 0.7.0's
programs predate: the tarball's 285 ELFs (the AppImage's 284 and
skwird), its skwird finding a name in a folder it indexed; the AppImage
carrying none of skwird. Not built: the whole of `just package-appimage`
with skwir's programs of now.
