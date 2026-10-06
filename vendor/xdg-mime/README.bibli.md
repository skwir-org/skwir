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
