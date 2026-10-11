# ADR-003: Embedded full-text search engine

Status: accepted, 2026-09-19. Amended by ADR-050 (skwird). Not built: the
`fulltext` plugin is planned for Phase 7.

## Context

The query language's `content:` term scans files with the grep crates:
correct and stateless, but linear in the subtree (about 27 µs per empty file
on the reference machine, dominated by opens; far more for large files). A
persistent keyword index would make repeated content searches instant.

Rejected: **Meilisearch**. A separate server with an HTTP API and its own
store; its engine crate is not a supported embedding target; its strengths
(typo tolerance, ranking rules, facets over structured documents) are not
what file content search needs. It conflicts with no daemon, no database,
and the memory budget.

## Decision

- **Tantivy** is the embedded engine: a Rust library, an index that is a
  directory of plain files, and the phrase, boolean and prefix queries a file
  manager needs.
- The index is **an accelerator, never a requirement**: one per subtree, built
  only on request, refreshed incrementally by mtime and content hash, kept in
  the deletable cache, pausable. Without an index covering the query root,
  the scan runs.
- Text extraction is shared with the Phase 7 `extract` provider, feeding
  keyword search, previews and later embeddings.
- **System indexes are providers.** Tracker, Baloo, Recoll, Spotlight,
  Windows Search, or a Meilisearch the user already runs may answer
  `content:`. None is ever installed or started by skwir.

## Consequences

- A Phase 7 `fulltext` plugin (`docs/PLAN.md`) owns the index lifecycle and
  its settings (roots, size caps, pause).
- The `query` plugin gains a provider hook for `content:`; the scan stays the
  fallback and the correctness reference in tests.
- Index size and build time become `skwir-bench` measurements when the plugin
  lands.

## Amendments

### 2026-10-10: skwird (ADR-050)

skwir may start one system index, skwird, for the session where it is
installed: a helper that ends with skwir, not a daemon. skwir still installs
and enables none.
