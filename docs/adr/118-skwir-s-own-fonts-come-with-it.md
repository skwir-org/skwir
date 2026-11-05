# ADR-118: Skwir's own fonts come with it

Status: accepted and built, 2026-11-05, on Linux (the runtime checked in
a window; the packages' scripts and checks written, not run: a release
runs them); macOS and Windows compiled, their bundles' scripts and
checks written. Amends ADR-073 (its "Not done: fonts a user adds to
Skwir alone").

## Context

- **"Skwir's own" became a choice** (ADR-117, Fonts from), and on macOS
  and Windows it is what Automatic means until their styles are read
  (ADR-071, step 5). It was Inter where the user had installed it, else
  DejaVu Sans, Noto Sans, Helvetica Neue or Segoe UI (`fonts.rs`), so it
  looked different on every machine; JetBrains Mono for code likewise.
  The README's screenshots are drawn in Inter (the flake provides it).
- **gpui draws a variable font's bold as its regular** (ADR-073, seen
  with Noto Sans), and Skwir draws medium, semibold and bold. Inter's
  own packages are often the variable font alone. (Since 2026-11-05 gpui
  patch 0045 draws a variable face's weights on Linux; the static files
  stay, drawn alike by every text system.)
- **Common practice is split.** Cross-platform applications with a look
  of their own carry their fonts (JetBrains' IDEs Inter and JetBrains
  Mono, Zed its own, Signal Desktop Inter); native Linux applications
  never do, and distributions remove copies, depending on their own font
  packages instead.
- Both fonts are under the SIL Open Font License 1.1, which allows
  shipping them with software, their licence beside them.
- Idle memory is over its budget (ADR-033: 103.7 MiB against 100 at
  0.3.0): what a window in the desktop's fonts never draws must cost it
  nothing.
- The user asked, 2026-11-05, whether to ship Inter, and took the
  recommendation below.

## Decision

1. **Every package built here carries Skwir's own fonts:** Inter
   (regular, italic, medium, semibold, bold, bold italic) and JetBrains
   Mono (regular, italic, bold, bold italic), static files, about 3.5 MB,
   in Skwir's data folder, `share/skwir/fonts`: above `bin` on Linux
   (the tarball and AppImage, the Debian and RPM packages, `skwir-bin`,
   the Nix package), beside the program on Windows, in a Mac's
   `Resources/share`. Their licences go with the others' (`licenses/fonts`,
   Debian's documents).
2. **From their projects' releases, pinned.** `tools/package/fonts.lock`
   names each archive with its SHA-256; `tools/package/fonts.py` fetches
   and checks them (or takes them already present, as the Nix package
   puts them, which may not fetch) and takes the static files and the
   licences out. The flake names the same archives and sums
   (`tools/release/test_fonts.py`).
3. **The AUR's source packages** (`skwir`, `skwir-git`), built on the
   user's machine, carry none: they list Arch's `inter-font` and
   `ttf-jetbrains-mono` as optional, found through fontconfig as any
   installed font.
4. **Read only when drawn, family by family, off the window's thread.**
   A carried family is needed by a role whose families (the one chosen,
   the desktop's, Skwir's own first) reach it before one that loads:
   Inter chosen and not installed, or Skwir's own where nothing chosen
   or lent loads (`fonts::needed`). At launch, before the window opens,
   a thread of its own reads the files of the families the settings and
   the desktop's style kept from the last run point to
   (`fonts::read_ahead`, `skwir_platform::fonts::bundled`); the window's
   first theme waits for it, so the first frame is drawn in them, and
   registers each file of a family needed and not installed, one at a
   time (`fonts::carry`), each family once a run. What was read and is
   not needed (an Inter installed) is let go. A family needed later
   (another choice, Fonts from on Skwir's own) is read on a thread too,
   the theme resolved again once it is in: the window never reads them.
   A window in the desktop's fonts reads and keeps none of them, one on
   macOS or Windows only JetBrains Mono for code; an installed Inter is
   used as it is. On a first run, or with the cache deleted, nothing
   says yet what the desktop lends: Skwir's own are read and kept for
   that run.
5. **Each package's check** looks for them and their licences
   (`linux/check.sh`, `appimage/check.py`, `macos-check.sh`,
   `windows-check.sh`). The AUR's source package carries none, so its
   check (`check.sh system-fonts`, as `try.sh arch` runs it) asks
   fontconfig for Inter and JetBrains Mono once Arch's optional packages
   are installed with what skwir recommends, and nothing of skwir alone.

Not decided here: whether Automatic on macOS and Windows becomes the
system's own interface font once their styles are read, Inter staying
Skwir's own. Decided 2026-11-05 (ADR-117): it is, San Francisco on a Mac
and Windows' message font (Segoe UI); code stays JetBrains Mono there.

## Consequences

- On macOS and Windows code is drawn in JetBrains Mono by default, and
  the window in Inter where Skwir's own fonts are chosen (until
  2026-11-05, by default too: Automatic is now the system's font).
- The packages grow by about 3.5 MB each; a window drawing Skwir's own
  fonts reads those of the families it draws once (Inter's 2.5 MB,
  JetBrains Mono's 1.1 MB), on a thread, and keeps them while it runs.
- A release fetches two more archives (from GitHub) beside PDFium's.
- Checked: the runtime on Xvfb with the files beside a debug build and
  Fonts from on Skwir's own: Inter and JetBrains Mono drawn ("fonts
  skwir carries count=10"), a Markdown file's bold, italic, bold italic
  and semibold heading drawn in their own faces; the platform's test of
  where a package puts them (Linux, Windows, a Mac's bundle, none for a
  build not packaged); `test_fonts.py`; the PKGBUILD tests. Not run: the
  packages' builds and checks, which a release runs.
