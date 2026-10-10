# ADR-003: Embedded full-text search engine

Status: accepted, 2026-09-19

## Context

The query language has a `content:` term that scans files with the grep
crates. That is correct and needs no state, but it is linear in the size
of the subtree (about 27 microseconds per file on the reference machine
for empty files, dominated by file opens, far more for large ones). A
persistent keyword index would make repeated content searches instant.

Meilisearch was considered. It is a separate server process with an HTTP
API and its own on-disk store, its engine crate is not a supported
embedding target, and its strengths (typo tolerance, ranking rules,
facets over structured documents) are not what file content search needs.
It conflicts with the principles of no daemon, no database, and the
memory budget.

## Decision

- **Tantivy** is the embedded full-text engine. It is a Rust library, its
  index is a directory of plain files, and it supports the phrase, boolean,
  and prefix queries a file manager needs.
- The index is an **accelerator, never a requirement**: one index per
  subtree, built only on request, refreshed incrementally by mtime and
  content hash, stored in the deletable cache, pausable. When no index
  covers a query root, the scan runs as today.
- Text extraction is shared with the Phase 7 `extract` provider, so the
  same content feeds keyword search, previews, and later embeddings.
- **System indexes are providers.** Tracker, Baloo, Recoll, Spotlight,
  Windows Search, and a Meilisearch instance the user already runs may
  serve the `content:` term when present. None is ever installed or
  started by bibli.

## Consequences

- A `fulltext` plugin in Phase 7 (see `docs/PLAN.md`), owning the index
  lifecycle and its settings (roots, size caps, pause).
- The `query` plugin gains a provider hook for the `content:` term so
  indexes and external services can answer it; the scan remains the
  fallback and the correctness reference in tests.
- Index size and build time become measured quantities in `bibli-bench`
  when the plugin lands.
- Amended (2026-10-10, ADR-050): bibli may start one system index,
  skwird, for the session, where skwird is installed: a helper that ends
  with bibli, not a daemon. It still installs and enables none.
