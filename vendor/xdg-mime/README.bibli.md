# Vendored xdg-mime 0.4.0

This directory is the `xdg-mime` 0.4.0 crate as published on crates.io
(Apache-2.0, `LICENSE.txt`), plus bibli's patch, wired in through
`[patch.crates-io]` in the root `Cargo.toml`. Remove both once the patch
is upstream (ebassi/xdg-mime-rs).

Left out: `test_files/`, the crate's own tests' data, which holds a copy of
shared-mime-info's database (GPL-2.0-or-later); bibli does not run this
crate's tests.

## Patch

Kept as a diff under `../patches/xdg-mime/`, in the upstream repository's
layout, so it applies there with `git apply -p1`.

- `0001-every-system.patch`: the crate no longer compiles to nothing off
  Unix. It reads the database's files by name and holds nothing of Unix's;
  on Windows bibli reads the database it ships beside the program, as the
  fallback of the system's own types (ADR-037).
- `0002-one-lowercasing-per-lookup.patch`: a name is looked up without
  an allocation per glob. `Glob::compare` lowercased the name again for
  every glob that ignores case, the matches were cloned, and each one's
  pattern was written to a `String` to measure it: 1,411 allocations to
  type `notes.txt` against the desktop's database, 6 now
  (`crates/platform/tests/mime_allocations.rs`).
- `0003-indexed-lookups.patch`: a name is matched against the patterns
  that can match it, not all of them. Literal patterns are found by the
  whole name, `*.ext` ones by each suffix of the name and of its
  lowercase form, and only the few full globs are tried one by one; the
  answer is the one trying every pattern gives (`lookup_linear`, which
  `crates/platform/tests/mime_index.rs` compares over the whole
  database): 22 µs a name to 1.2 (`mime_by_name_us`). Aliases are a map,
  the first directory to name one deciding it as before. A subclass
  cycle in a database ends instead of overflowing the stack. And
  `new_for_directories` reads the database from the directories its
  caller found: bibli reads the XDG variables in one place
  (`bibli_core::xdg`).
