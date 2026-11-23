# skwird's client, copied

`skwird-client` from skwird, the index of names that is a project of its
own (ADR-050). It finds an engine (the system service's socket, then the
user's) or starts `skwird serve` from the `PATH`, and asks it; it also
holds a fake engine for clients' tests (`src/fake.rs`). MIT OR
Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`). `VENDORED` names the
skwird commit it was copied from.

- **Published nowhere.** skwird publishes no crate on crates.io (the
  user's choice, 2026-11-11) and Skwir takes no git dependency, so Skwir
  keeps this copy, and `../skwird-protocol` beside it, as it keeps gpui
  (ADR-050, amended 2026-11-23).
- **Not Skwir's code.** It is outside the workspace (`exclude` in the
  root `Cargo.toml`) and taken by path, as `skwird-client` in
  `[workspace.dependencies]`: Skwir's lints and tests do not run on it.
  It carries no patch. A change is made in skwird, then copied again.
- **Updating.** From Skwir's root, with a checkout of skwird:
  `<skwird>/tools/vendor-client vendor`. It writes both copies again,
  replacing them, and `VENDORED` says so if skwird had changes not
  committed there. Read the diff: the protocol is experimental until
  skwird declares it stable, and a version may change meanwhile. Then run
  the tests of what takes it.
- **Dependencies.** The protocol's copy, by path. serde and serde_json
  come from crates.io through Skwir's lockfile, held to the advisories
  as Skwir's own (`just deny`). `tools/deny/vendored.py` holds only the
  copies that stand for a release; this one stands for none.
- **Every system.** Off Unix, `Client::connect` answers `NoEngine`,
  naming the system: skwird runs on Linux so far (its phases 4 and 5 are
  Windows and macOS). On macOS it finds no engine and starts none that
  answers.
