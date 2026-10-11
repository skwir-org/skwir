# ADR-039: Isolating converters on macOS and Windows

Status: accepted, 2026-10-06 (Phase 5; ADR-035). Extends ADR-030 and
ADR-031, whose rule stands: a converter a file can make do anything runs
isolated or not at all. Built: the macOS profile (compiled, never run),
Pandoc's `--sandbox`. Not built: Windows.

## Context

Skwir runs programs on untrusted files as soon as one is selected.
LibreOffice, Pandoc and ImageMagick must run isolated
(`sandbox::Otherwise::Refuse`); decoders the desktop runs on the same
files anyway (ffmpeg, Poppler's tools, Skwir's helpers, thumbnailers)
are isolated where possible (`Otherwise::Run`). Linux uses bubblewrap;
elsewhere the converters were off and the decoders unisolated.

- **macOS: Seatbelt** (`sandbox-exec` with a profile). Deprecated for
  years but shipped; used by Chromium, Bazel, Nix and coding agents.
  Costs milliseconds.
- **Windows: AppContainer** (Edge and Chromium renderers, Office's
  Protected View): no capabilities (no network), opens only what grants
  it access. Unknown whether LibreOffice, or a Pandoc in the user's
  folder, starts in one. Rejected: a low-integrity token (blocks writes,
  not reads or network); Windows Sandbox (a VM, seconds per start, some
  editions only).
- **Pandoc** has `--sandbox` since 2.15 (reads only named files, fetches
  nothing) and is memory-safe. LibreOffice and ImageMagick parse hostile
  formats in C/C++, and their macros, links and delegates have each let
  a file take over the program.

Cost does not decide: milliseconds beside a converter's start
(LibreOffice: 1-3 s).

## Decision

1. **macOS isolates with `sandbox-exec`** (`platform::sandbox`) under
   bubblewrap's contract: no network; reads system folders, installed
   packages, the program (and its `.app`), the input and its working
   folder; writes only there; may see where files are but not read
   them, and ask for fonts, the user's name and preference defaults,
   which programs need to start. The profile names real paths, so the
   input is linked into the working folder under a plain name;
   `sandbox::command` returns the paths to pass (on Linux `/input/…`,
   `/work`). `sandbox::available` checks the profile runs; if Apple
   removes the command, the converters are off.
2. **Windows will use an AppContainer**, built and tried on a Windows
   machine (5b), not written blind. Until then Windows isolates nothing:
   LibreOffice and ImageMagick are off, decoders run as is.
3. **Pandoc always runs with `--sandbox`** (2.15+), on every system,
   inside the system's sandbox too, so it runs even where nothing else
   isolates it. An older Pandoc runs only system-isolated. It writes
   page and pictures relative to its working folder, so names match
   wherever it ran.
4. **LibreOffice and ImageMagick never run unisolated** (unchanged).
5. **System thumbnails first** where they exist (Quick Look, Windows'
   thumbnail cache), already isolated, so ImageMagick matters less. A
   direction for step G, not built here.

## Consequences

- On macOS everything runs isolated once a Mac confirms each program
  starts under the profile (compiled, never run; 5b). Its allowances
  (fonts, name, preferences, file locations) are to be narrowed if
  programs start without them.
- On Windows, Office documents are shown by Skwir's readers and Pandoc,
  not as pages, until AppContainer is built.
- A plugin's isolation requirement names the system's program
  (`sandbox::PROGRAM`: `bwrap`, `sandbox-exec`; none on Windows).
