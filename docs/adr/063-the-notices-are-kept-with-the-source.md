# ADR-063: The packages' notices are kept with the source

Status: accepted, 2026-10-17. Revises how the notices of 2026-10-07
(PLAN, Phase 5 step 8) are made; what each package installs is unchanged.

## Context

- Every Linux package installs `THIRD-PARTY-NOTICES`: each crate its
  programs link, under the licence it is used under, with the text the
  crate carries (MIT, BSD, ISC and Zlib ask for their notices in a
  binary's distribution). `tools/package/notices.py` made them with
  cargo-about during each package's build: Debian's, Fedora's, the
  AppImage's and Arch's.
- An Arch build of 0.1.0 in a clean environment stopped there (reported
  2026-10-16): "failed to download `android_system_properties v0.1.6`
  … --offline was specified". The PKGBUILD fetches only its own
  platform's crates (`cargo fetch --target`, Arch's Rust guideline);
  cargo-about runs `cargo metadata` without `--filter-platform`, which
  reads every crate `Cargo.lock` names, Android's, Windows' and macOS'
  included, before cargo-about keeps Linux's (`targets` in `about.toml`;
  its source, 0.9.2). Checked on a project depending on `iana-time-zone`
  alone, with an empty `CARGO_HOME` and the host's crates fetched:
  `cargo metadata --offline` fails with that error, and succeeds with
  `--filter-platform` (one triple or two). Builds that worked had every
  crate in a shared cache (`try.sh` binds the host's).
- The notices depend only on `Cargo.lock`, the crates' own files (checked
  by their checksums) and which programs each package ships. Each
  packager made the same files again: cargo-about built in every build
  root (`cargo install`), about 200 more crates fetched (78 MB of 195),
  and some forty CPU-minutes per build (2 min 43 s on 20 threads).
- Other tools: cargo-bundle-licenses reads the whole graph too;
  cargo-tribute (July 2026) passes `--filter-platform` but writes each
  licence's SPDX text once with the copyright lines found, not the
  crates' own texts, and is three months old with one maintainer.

## Decision

1. **The notices are made once and kept with the source**, in
   `tools/package/notices/<package>/THIRD-PARTY-NOTICES`, by
   `just notices` (cargo-about, as before). The packages copy them;
   no package build runs cargo-about.
2. **A split of its own has a file of its own.** Debian's, Fedora's and
   the AppImage's packages are the split `tools/package/optional` names.
   Arch's keeps apart only the packages its PKGBUILD's `pkgname` lists, so
   its `skwir`, which holds the media, Poppler and DjVuLibre helpers, has
   `arch/skwir`; its other packages are the same as Debian's and use
   those.
3. **`notices.py` fetches every platform's crates, then reads offline**:
   online, cargo-about looks on the network for a licence a crate leaves
   out, and the notices would depend on what it found that day.
4. **`just notices-check`, in `release-check`, refuses stale notices**: it
   makes them again and compares.

## Consequences

- A dependency added or updated, or a helper shipped in another package,
  means `just notices` and the changed notices committed with it
  (`AGENTS.md`); the release gate says when it was forgotten.
- The repository holds about 1.5 MB of generated text (seven files),
  which changes with the dependencies.
- A packager needs no cargo-about and no other platform's crates: the
  PKGBUILD's `cargo fetch --target` is enough again.
- `release-check` takes about three minutes more (on the machine that
  measured the 2 min 43 s).
- The macOS and Windows packages, when written, add their splits and
  targets to `notices.py` and keep their notices beside these.
- 0.1.0's source predates this; its AUR package needs its PKGBUILD
  fetching every platform's crates until the next release.
- cargo-about could pass `--filter-platform` for its `targets`, which
  would let anyone package offline with it (not reported upstream yet);
  these notices would stay kept all the same.
