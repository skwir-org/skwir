# Vendored syntect 5.3.0

The `syntect` 5.3.0 crate as published on crates.io (MIT, `LICENSE.txt`),
plus Skwir's patch, wired in through `[patch.crates-io]` in the root
`Cargo.toml`. Remove both once the patch is upstream
(trishume/syntect) or a release offers the same engine.

Left out: the repository's own files the crate carries (`.github/`,
`.gitattributes`, `.gitignore`, `.gitmodules`, `.git-blame-ignore-revs`)
and the registry's `.cargo-ok`. Skwir does not run this crate's tests.

## Patches

Diffs in `../patches/syntect/`, in the upstream repository's layout (its
`Cargo.toml` is the published `Cargo.toml.orig`), so they apply there with
`git apply -p1`. The published, normalized `Cargo.toml` here carries the
same change.

- `0001-ferroni-engine.patch`: a third regex engine, `regex-ferroni`, on
  Ferroni (a pure-Rust port of Oniguruma, BSD-2-Clause), with the onig
  backend's syntax (Oniguruma's own, `OnigDefaultSyntax`) and options. It
  wins over `regex-onig` and `regex-fancy` when several are enabled. A
  pattern Ferroni refuses never matches instead of panicking its
  grammar: of the 22,257 patterns in bat's grammars, Ferroni 2.1.3
  refuses two that Oniguruma 6.9.10 takes, both in Vue Component, whose
  class `[^ "'>/=\x00-\x1f\x7f-\x9f]` holds byte escapes past `\x7f`.
  Oniguruma's last source, after 6.9.10, refuses those in UTF-8 too, and
  Ferroni ports it (ferroni#227); 6.9.10 took them as bytes that never
  match valid UTF-8. Every other pattern gives the same scopes as
  Oniguruma. Why Ferroni, and what was measured: ADR-060.
