# Vendored two-face 0.5.2+bat-0.26.1

The `two-face` 0.5.2 crate as published on crates.io (MIT OR Apache-2.0,
`LICENSE-MIT`, `LICENSE-APACHE`): bat 0.26.1's syntax and theme
definitions for syntect, plus Skwir's patch, wired in through
`[patch.crates-io]` in the root `Cargo.toml`. Remove both once the patch
is upstream (codeberg.org/CosmicHarper/two-face).

Left out: the registry's `.cargo-ok`. Skwir does not run this crate's
tests.

## Patches

Diffs in `../patches/two-face/`, in the upstream repository's layout (its
`Cargo.toml` is the published `Cargo.toml.orig`), so they apply there with
`git apply -p1`. The published, normalized `Cargo.toml` here carries the
same change.

- `0001-ferroni-feature.patch`: a `syntect-ferroni` feature, which turns
  on syntect's `regex-ferroni` engine (`../syntect/README.skwir.md`) and
  selects Oniguruma's syntax definitions, all of bat's: the fancy-regex
  set leaves out six that fancy-regex cannot compile (ARM Assembly,
  JavaScript (Babel), LiveScript, PowerShell, Sass, Salt State SLS).
  ADR-060.
