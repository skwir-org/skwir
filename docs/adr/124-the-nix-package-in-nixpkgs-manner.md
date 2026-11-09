# ADR-124: The Nix package in nixpkgs' manner, in variants

Status: accepted 2026-11-08 with the user, from issue #7 (the package's
closure was 3 GB); built on Linux (x86_64), aarch64-linux and
aarch64-darwin evaluated only. Amends ADR-040 (a row for Nix), ADR-041
(nixpkgs has PDFium), ADR-042 (the licences follow the flags), ADR-064
(the git pins in a nixpkgs copy) and ADR-118 (the font archives named in
the package).

## Context

- **The closure was 3 GB, 2.6 GiB of it the Rust toolchain.** The
  package was built with the dev shell's toolchain (rust-src,
  rust-analyzer, clippy, rustfmt, llvm-tools, the docs and three cross
  targets). With rust-src in its sysroot, rustc writes the paths of std's
  sources into the panic messages of the code it inlines, so every
  binary named the toolchain's store path (6 to 62 such strings each,
  kept by `strip`), and Nix's reference scan kept all of it.
- **Everything else was all or nothing:** GStreamer with every plugin
  set (about 700 MiB of closure, plugins-bad alone pulling a long list),
  Poppler and MuPDF beside PDFium's helper, which found no PDFium on
  NixOS (none carried, none on any path), so `auto` fell back to Poppler.
  Every package elsewhere splits these (ADR-041: `skwir-media`
  recommended, Poppler and MuPDF suggested).
- **Also:** the `release` profile (debug = 1), thirteen helpers on the
  users' PATH, and `LD_LIBRARY_PATH` set by the wrapper for the window's
  libraries, which every program skwir starts inherited.
- **Nix has no "recommends":** what an output references is in its
  closure, so what is optional is chosen when the package is built. A
  library already on the machine from the same nixpkgs is one store
  path, kept once (`follows`).
- **The standard:** nixpkgs makes a package a function with an argument
  per optional part (`withX ? true`), its default sensible, its other
  forms `.override` or ready-made attributes (`gitMinimal`/`gitFull`,
  `ffmpeg-headless`/`ffmpeg-full`). nixpkgs builds PDFium from source
  (`pdfium`, since it replaced the prebuilt `pdfium-binaries` on
  2026-08-23) and its applications name it from a wrapper (stump:
  `--set-default PDFIUM_PATH`).

## Decision

1. **One function, `tools/package/nix/package.nix`,** written as
   nixpkgs writes a package, but for three arguments the flake gives:
   `src` (its git tree), `version` (the workspace's) and `cargoLock` (the
   lock file, its git pins fetched as it names them). A copy in nixpkgs
   gives `fetchFromGitHub`, a literal version and `cargoHash` instead.
2. **A flag per helper with libraries of its own, or that few want,**
   each default the engine `auto` uses:

   | Flag | Default | Brings |
   |---|---|---|
   | `withPdfium` | on | the PDF helper, nixpkgs' `pdfium` named by the wrapper (`SKWIR_PDFIUM_LIBRARY`; a setting naming another still wins) |
   | `withPoppler` | off | Poppler's reader (poppler-glib, cairo, glib) |
   | `withMupdf` | off | MuPDF's reader (bundled, AGPL) |
   | `withDjvulibre` | on | DjVuLibre's reader (djvu-rs's stays) |
   | `withArchive` | on | archives (libarchive) |
   | `withMermaid` | on | Mermaid diagrams (no library, large) |
   | `withMedia` | on | the player (GStreamer and its base plugins) |
   | `gstPlugins` | base, good, bad, ugly, libav | the player's formats: `ps: [ … ]`, of the package's own GStreamer; base always added |

   The helpers with nothing of their own (djvu-rs, the 3D model reader,
   modules, MTP, tree-sitter, print) are in every variant. The flag names
   are `passthru.features`, the one list the variants come from.
3. **Three variants:** `default` (the table's defaults), `minimal` (every
   flag off) and `full` (every flag on); anything else is
   `.override { … }`. The overlay and the Home Manager module take
   `default`; `programs.skwir.package` takes any.
4. **The toolchain stays out:** the flake builds the package with the
   pinned Rust's `minimal` profile (rustc, cargo, std: no rust-src), and
   the package refuses to reference either (`disallowedReferences`).
5. **Laid out as the other packages:** `skwir` in `bin`, its helpers in
   `libexec/skwir` (`process::helper_folders`); the `dist` profile; the
   window's libraries added to skwir's own search path
   (`patchelf --add-rpath`, after fixup has shrunk it), so nothing skwir
   starts inherits them; the wrapper sets only where PDFium and the
   player's plugins are. Fonts, desktop entry, metainfo and icon as
   before; `LICENSE`, and the notices of skwir and of each helper the
   build carries, one file (ADR-063).
6. **Checked:** `tools/release/test_nix.py` (release-tools-check) keeps
   the package's programs, flags and notices alike with
   `tools/package/programs` and `optional`;
   `just package-nix-check` (tools/package/nix/check.sh) builds
   `minimal` and `default` from scratch, checks every helper answers,
   the closure holds no toolchain and no library of a helper left out,
   and prints each closure. A release step, not a routine check.

## Consequences

- Closures, measured on x86_64-linux: see the STATUS-LOG entry of
  2026-11-08 (default, minimal; full evaluated).
- Nix users choose: `.override { withMedia = false; }`, `minimal`,
  `full`; with `inputs.skwir.inputs.nixpkgs.follows = "nixpkgs"` the
  libraries are the ones their system already has (untested there: the
  lock is what is built here).
- PDFs open through PDFium on Nix; Poppler and MuPDF show as not
  installed until their flag is on, as on Debian without their packages.
- `minimal` reads no PDF (thumbnails from `pdftoppm` where Poppler's
  tools are on the PATH, ADR-041), opens no archive, plays nothing.
- A flag changed is another derivation, built from source: there is no
  binary cache for skwir.
- aarch64-linux and aarch64-darwin are evaluated, never built here.
