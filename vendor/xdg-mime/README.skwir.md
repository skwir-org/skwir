# Vendored xdg-mime 0.4.0

The `xdg-mime` 0.4.0 crate as published on crates.io (Apache-2.0,
`LICENSE.txt`), plus Skwir's patches, wired in through `[patch.crates-io]`
in the root `Cargo.toml`. Remove both once the patches are upstream
(ebassi/xdg-mime-rs).

Left out: `test_files/`, the crate's test data, which holds a copy of
shared-mime-info's database (GPL-2.0-or-later). Skwir does not run this
crate's tests.

## Patches

Diffs in `../patches/xdg-mime/`, in the upstream repository's layout, so
they apply there with `git apply -p1`.

- `0001-every-system.patch`: compiles off Unix instead of to nothing. The
  crate reads the database's files by name and needs nothing of Unix; on
  Windows Skwir reads the database it ships beside the program, as the
  fallback of the system's own types (ADR-037).
- `0002-one-lowercasing-per-lookup.patch`: no allocation per glob.
  `Glob::compare` lowercased the name again for every case-insensitive
  glob, cloned the matches, and wrote each pattern to a `String` to measure
  it. Typing `notes.txt` against the desktop's database took 1,411
  allocations, now 6 (`crates/platform/tests/mime_allocations.rs`).
- `0003-indexed-lookups.patch`: a name is matched only against the
  patterns that can match it: literals by the whole name, `*.ext` by each
  suffix of the name and of its lowercase form; only the few full globs
  are tried one by one. The answer equals trying every pattern
  (`lookup_linear`, compared over the whole database by
  `crates/platform/tests/mime_index.rs`): 22 µs a name, now 1.2 µs
  (`mime_by_name_us`). Also:
  - aliases are a map; the first directory to name one still decides;
  - a subclass cycle ends instead of overflowing the stack;
  - `new_for_directories` reads the database from the directories its
    caller found, since Skwir reads the XDG variables in one place
    (`skwir_core::xdg`).
