# skwird's protocol, copied

`skwird-protocol` from skwird (ADR-050): the messages a client and an
engine say to each other, as JSON lines, which skwird's engine speaks
too. `transcripts/session.jsonl` is a session of every message, which
the crate's own test holds to its types. MIT OR Apache-2.0
(`LICENSE-MIT`, `LICENSE-APACHE`). `VENDORED` names the skwird commit it
was copied from.

It is copied with `../skwird-client`, by the same script, and kept the
same way: published nowhere, outside the workspace, never patched here,
serde and serde_json its only dependencies. See
`../skwird-client/README.skwir.md`.

The protocol is version 0, experimental until skwird declares it stable.
A copy older than the engine it talks to may fail to read an answer: a
reason added to `Why` (`snapshot`, skwird's 6e418d7) is a variant this
crate must know. The client's copy then gives an error, and Skwir takes
every error as no engine, and walks (ADR-050, point 1).
