# ADR-039: Isolating converters on macOS and Windows

Status: accepted, 2026-10-06 (Phase 5; ADR-035). Extends ADR-030 and
ADR-031, whose rule stands: a converter that a file can make do
anything it can runs isolated, or not at all.

## Context

bibli runs other programs on files nobody vouched for, as soon as one
is selected: LibreOffice, Pandoc and ImageMagick (which must run
isolated, `sandbox::Otherwise::Refuse`), and decoders the desktop runs
on the same files anyway (ffmpeg, Poppler's tools, its own helpers, the
desktop's thumbnailers: isolated where possible, else run as they are,
`Otherwise::Run`). On Linux, bubblewrap isolates them: no network, the
system's folders and the input readable, a disposable folder writable.
macOS and Windows have no bubblewrap, so until now the converters were
off there and the decoders ran unisolated.

What each system offers:

- **macOS**: Seatbelt, the kernel's sandbox, through `sandbox-exec` and
  a profile in its own language. Apple has marked the command
  deprecated for years, ships it still, and Chromium, Bazel, Nix and
  coding agents run programs under it. Its cost is a profile compiled
  at start, milliseconds; its checks are the kernel's.
- **Windows**: an AppContainer, the isolation of Edge's and Chromium's
  renderers and of Office's Protected View: a token with no
  capabilities (no network), able to open only what grants it access
  (its working folder; the input copied in, or handed open). Its cost
  is a profile made once and a usual process start. Whether LibreOffice,
  or a Pandoc installed in the user's folder, starts inside one is
  unknown until it is tried on Windows. A restricted token at low
  integrity blocks writes but neither reads nor the network: too weak.
  Windows Sandbox is a virtual machine: seconds per start, and only in
  some editions.
- **Pandoc** has a sandbox of its own since 2.15 (`--sandbox`): its
  readers and writers read only the files it is named, and fetch
  nothing; it is written in a memory-safe language. LibreOffice and
  ImageMagick have nothing of the kind, and parse hostile formats in C
  and C++ (LibreOffice's macros and links, ImageMagick's delegates have
  each been how a file took over the program).

Performance is not what decides: either sandbox costs milliseconds
beside a converter's start (LibreOffice's, one to three seconds).

## Decision

1. **macOS isolates with `sandbox-exec`** (`platform::sandbox`), under
   bibli's profile, with the contract bubblewrap has on Linux: no
   network; it reads the system's folders, what a package manager
   installed, the program itself (its `.app` with it), the input and its
   working folder; it writes only in that folder; it may read where
   things are but not what they hold, and ask the system for fonts,
   the user's name and the preferences' defaults, which programs need
   to start. Paths are the real ones, which the profile names, so the
   input is reached through a link in the working folder under a plain
   name, and `sandbox::command` returns the paths a program is to be
   given (on Linux `/input/…` and `/work`) instead of every caller
   assuming bubblewrap's. `sandbox::available` asks whether the profile
   runs on this macOS; if Apple removes the command, the converters are
   off again, as on a Linux without bubblewrap.
2. **Windows will isolate with an AppContainer**, built and tried on a
   Windows machine (5b): not written blind, since what decides is
   whether the converters start inside one. Until then Windows isolates
   nothing: LibreOffice and ImageMagick are off there, the decoders run
   as they are.
3. **Pandoc always runs in its own sandbox** where it has one
   (`--sandbox`, Pandoc 2.15 or later), on every system, inside the
   system's sandbox too. With it, Pandoc runs where the system isolates
   nothing (Windows; a Linux without bubblewrap): its own sandbox keeps
   it to the file it is given. A Pandoc too old for it runs only
   isolated by the system, as before. It writes its page and pictures
   relative to its working folder, so the page names them the same
   wherever it ran.
4. **LibreOffice and ImageMagick never run unisolated**, on any system:
   unchanged.
5. **The systems' own thumbnails come first where they exist** (macOS's
   Quick Look, Windows' thumbnail cache), which the systems already run
   isolated, so ImageMagick matters less there: a direction for step G
   on the machines, not built here.

## Consequences

- On macOS LibreOffice, Pandoc, ImageMagick and the decoders run
  isolated once a Mac confirms the profile lets each of them start: the
  profile is compiled, never run (5b), and what it lets through (the
  font, name and preference services; where any file is) is what
  programs ask to start, to be narrowed if they start without it.
- On Windows Office documents are shown by bibli's own readers and
  Pandoc, not as pages, until AppContainer is built.
- The requirement a plugin lists for isolation names the program that
  isolates on that system (`sandbox::PROGRAM`: `bwrap`, `sandbox-exec`),
  and none on Windows.
