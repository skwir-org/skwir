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
  as Skwir's own (`just deny`); on Linux, rustix (its `fs` and `net`),
  for a connect that a full socket queue cannot hold past its silence or
  a stop (from skwird 0.2.0). `tools/deny/vendored.py` holds only the
  copies that stand for a release; this one stands for none.
- **Every system.** On Windows the client reaches an engine over the
  user's named pipe (`\\.\pipe\skwird-<SID>`, or `SKWIRD_PIPE`),
  checks that the pipe's server runs as the same user before saying
  anything, and starts an engine without a console (ADR-074), through
  windows-sys (its only dependency there, from Skwir's lockfile). On
  macOS it looks for the system service at `/var/run/skwird/socket` and
  for the user's engine in the user's own temporary folder, as the engine
  serves there (macOS sets no `XDG_RUNTIME_DIR`; libc's `confstr`, its
  only dependency there). skwird 0.2.0 ships engines for Windows and
  macOS, tried on GitHub's runners; Skwir has asked neither, only
  skwird's fake engine under Wine.
- **Stopped.** A connect is stopped with a `Stopper` made before it
  (`Client::connect_stopped`): its hello and a started engine's wait end
  at once. Skwir's plugin stops it from the search's token and after a
  few seconds without a hello, by a timer of its own (`alarm.rs`).
