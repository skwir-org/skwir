# Contributing to Skwir

Skwir is young: a report of what went wrong is as welcome as code. If you
have a Mac or a Windows machine, [the steps towards those
releases](docs/HELP-WANTED.md) need you most.

## Reporting a problem

Open an issue in [the tracker](https://skwir.org/issues) with:

- your system and its version (More › About Skwir › Copy details);
- what you did, what you expected, and what happened;
- the log if you can (`RUST_LOG=skwir=debug skwir` from a terminal).

**Rename anything personal before you share it.** Paths, file names and
screenshots say a lot about you: a folder called `taxes 2026` can be
`folder a` in a report.

A security problem (a private folder reached, a plugin getting what it
did not declare, a way around a plan's review) goes to
[SECURITY.md](SECURITY.md)'s address, not to an issue.

## Building and checking

The dev environment is the Nix flake (`nix develop`, or `direnv allow`).
Without Nix: Rust 1.96 or later and the libraries `flake.nix` lists. The
checks run the Rust that `rust-toolchain.toml` pins, which rustup
installs by itself; moving the pin is a change of its own, with the
fixes the new clippy asks for.

```
just build          # Skwir without its native helpers
just run [folder]   # the release build, with every helper
just test           # the tests, through cargo-nextest
just lint           # rustfmt and clippy, warnings as errors
```

A change is done when CI's checks pass: `just lint-all`, `just deny`,
`just test-all`, `just smoke`, `just bench-check`, `just poppler-check`
and `just mupdf-check` (`AGENTS.md`, "Build, test, lint").

## The rules the code follows

[`AGENTS.md`](AGENTS.md) holds them, for people and agents alike: the
principles (always responsive, light, stateless, a small core, ready for
agents), the engineering rules, and how to build and test. The rules of
each larger area (previews, media, the reader…) are in
[`docs/invariants/`](docs/invariants/). Read `AGENTS.md` once before a
first change, and the area's file before working there.

A decision that closes a door gets an ADR in [`docs/adr/`](docs/adr/).
[`docs/PLAN.md`](docs/PLAN.md) records what is done and what is left.

Two rules matter from the first commit:

- **Nothing personal enters the repository.** Tests, documents,
  screenshots, logs and commit messages use synthetic names that keep
  what matters (an apostrophe, a space, a multi-byte character).
- **Tests use real filesystems under `~/.cache/skwir-tests`**, never your
  own files, and run through nextest, which keeps them out of your Trash,
  settings and cache (`tools/test/isolate-xdg.sh`).

## Licence

Skwir is under the Apache License 2.0. What you contribute is under it
too (section 5 of the licence); there is no separate agreement to sign.
