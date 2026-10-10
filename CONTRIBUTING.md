# Contributing to skwir

Thank you for helping. skwir is young, so a report of what went wrong is
as welcome as code. With a Mac or a Windows machine, [the steps towards
those releases](docs/HELP-WANTED.md) need you most.

## Reporting a problem

Open an issue with your system and its version (More › About skwir ›
Copy details copies them), what you did, what you expected and what
happened, and the log if you can (run
`RUST_LOG=skwir=debug skwir` from a terminal). **Rename anything personal
before you share it**: paths, file names and screenshots say a lot about
you, and a folder called `taxes 2026` can be `folder a` in a report.

A security problem (something that reads a private folder, a plugin that
gets what it did not declare, a way around a plan's review) goes to
[SECURITY.md](SECURITY.md)'s address instead, not to an issue.

## Building and checking

The dev environment is the Nix flake (`nix develop`, or `direnv allow`);
without Nix, Rust 1.96 or later and the libraries `flake.nix` lists. The
checks run the Rust `rust-toolchain.toml` pins, which rustup installs by
itself; moving the pin is a change of its own, with what the new clippy
asks.

```
just build          # skwir without its native helpers
just run [folder]   # the release build
just test           # the tests, through cargo-nextest
just lint           # rustfmt and clippy, warnings as errors
```

Before a change is done, CI's checks pass: `just lint-all`, `just deny`,
`just test-all`, `just poppler-check`, `just mupdf-check` and `just
bench-check` (`AGENTS.md`, "Build, test, lint").

## The rules the code follows

[`AGENTS.md`](AGENTS.md) holds them, for people and agents alike: the
principles (always responsive, light, stateless, a small core, ready for
agents), the engineering rules, how the UI is laid out, and the rules of
each area. Read it once before a first change; it is long because it
says what the code would otherwise teach by breaking. A decision that
closes a door gets an ADR in [`docs/adr/`](docs/adr/), and
[`docs/PLAN.md`](docs/PLAN.md) says honestly what is done and what is
left.

Two rules matter from the first commit:

- **Nothing personal enters the repository**: tests, documents,
  screenshots, logs and commit messages use synthetic names that keep
  what matters (an apostrophe, a space, a character of several bytes).
- **Tests use real filesystems under `~/.cache/skwir-tests`**, never your
  own files, and run through nextest, which keeps them out of your Trash,
  settings and cache (`tools/test/isolate-xdg.sh`).

## Licence

skwir is under the Apache License 2.0. What you contribute is under it
too (the licence's section 5): no separate agreement to sign.
