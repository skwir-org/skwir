# ADR-052: A portable AppImage built in a distribution root

Status: accepted, 2026-10-11 (the user requested the implementation).
Amends ADR-040's Linux formats and Phase 5's packaging deviation.

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
   Build pinned GStreamer 1.28.8 core and base plugins, with Orc 0.4.44,
   from unmodified sources against this baseline: Ubuntu's 1.20 playback
   engine fails the current worker's stream selection. Other distribution plugins
   use the stable GStreamer 1.x ABI. Replace the old core/base plugins by
   source-package ownership, including plugins renamed upstream. Ship the
   exact source archives, including their licence notices, beside the
   native library notices.
3. **Library paths stay inside the package.** Each ELF's RUNPATH is relative
   to its executable or library; it does not impose an inherited RPATH on
   host graphics drivers. AppRun exports no loader or search paths
   to the editor, terminal or converters. Only the media worker's wrapper
   selects the bundled GStreamer plugins and scanner. No decoder tuning
   or hardware selection is added.
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

## Consequences

- `just package-appimage` writes the AppImage and its SHA-256 file to
  `target/package/appimage`. Build roots and artifacts stay under
  `target/appimage`; deleting that folder resets the build.
- Building on a Nix host is supported. Running a conventional AppImage
  on NixOS may still need `appimage-run` for its FHS runtime.
- The bundled player and alternative readers make this larger than
  the main Debian package. Their GPL/AGPL licences and native library
  notices ship with them; source remains in the release's repository.
- System tools remain optional. Bubblewrap and usable user namespaces
  are host requirements for features that require isolation, as before.
- The Debian, RPM, AUR and Nix packages remain available. AppImage is an
  additional format, with bundled-library updates our responsibility.

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
