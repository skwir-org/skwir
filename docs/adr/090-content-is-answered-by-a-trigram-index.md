# ADR-090: `content:` is answered by a trigram index of what a file reads as text; Tantivy is dropped

Status: accepted, 2026-10-26, the user's choice
(`docs/indexing-and-ai.md`). Not built.
Revises ADR-003 (Tantivy as the embedded engine) and builds on
ADR-050's coverage (an index that proves its coverage answers).

## Context

- ADR-003 chose Tantivy for a Phase 7 `fulltext` plugin: an index per
  subtree, built on request, an accelerator never a requirement, system
  indexes as providers. Nothing was built.
- `content:` is a grep: a pattern or a regex, case as `case:` says, a
  file read up to its first NUL, a UTF-16 byte order mark decoded first
  (`lang::content_matches`). Tantivy tokenizes and ranks; it cannot
  answer a regex or a substring as the scan does, and its answer would
  differ from the scan's, which is the correctness reference in tests.
- `crates/bench/src/trigram.rs` built and timed a trigram index of what
  `content:` reads, planned from the regex's HIR so that it names every
  file that can match, each then matched by the searcher itself, and
  checked query by query against the scan. On the reference home: 11 %
  of the text; queries 5 to 175 times faster than the scan, which took
  34 to 67 s.
- A document (PDF, Office, EPUB, email) is matched today on its bytes,
  which is near useless; its text is what the preview reads
  (`docs/invariants/previews.md`), and ADR-091 keeps that text.

## Decision

1. **The `fulltext` plugin keeps a trigram index** of `extract.text`
   (ADR-091), one per root under the harvest's scope (ADR-092), in the
   deletable cache, in the bench's shape: segments of about 32 MB of
   text, each a document table (file, version, length, its parts'
   offsets), its trigrams sorted with a posting list each, and
   tombstones; a header with the layout version and every array's
   width. Segments are added and merged in the background; the index is
   never rewritten whole. Damaged, older or of another width, it is
   rebuilt.
2. **`content:` matches what a file reads as text:** its bytes for a text
   file, as today; its extracted text for a document, read by the
   preview's readers under the preview's bounds. The index names the
   files that can match; `content_matches` decides on the text: a text
   file's bytes read again, a document's text as the extract provider
   keeps it (`Fields::get`, extracted again if it was let go of). Where
   no index covers, the scan runs as the reference and extracts on the fly
   under the same bounds: slow there, never wrong, never different. A
   second term for documents' text (`text:`) was considered and
   rejected: a user searching for a word expects the PDF holding it.
3. **Coverage, not hints.** The plugin is a `HintProvider` with a proof
   (ADR-050's step A, `harvest::Proof`). Under a root whose proof is
   live or checked, the search considers only what the index names and
   does not scan; its footer says as of when and how many files are not
   read. Elsewhere it hints and scans. A pattern that leaves no trigram
   (two letters, a class too wide) scans.
4. **Tantivy is not used.** Ranking by meaning is ADR-094's; keyword
   relevance has no term. ADR-003's other points stay: the system's
   indexes may answer `content:` as hint providers, none is installed
   or started by Skwir.

## Consequences

- One engine, one semantics, one correctness reference: the bench's
  query-by-query check becomes a test of the plugin.
- The index pays most for rare words and regexes holding literals (a
  rare word 0.19 s against the scan's 34 s on the reference home) and
  least for a word nearly every file holds, whose files must all be
  read to be matched (common words and a regex 5.8 to 7.2 s against 40
  to 67 s). A pattern too short for a trigram scans.
- A scan over a folder of documents that no index covers is slower than
  it was (it extracts) and right; the footer's "Searching…" counts as
  before. The harvest makes it rare where the user turned indexing on.
- `skwir-bench trigram` gains documents' text and the segment format;
  budgets `trigram_segment_us` and `trigram_query_us`.
- The `fulltext` plugin costs nothing idle: segments are memory-mapped
  when a search needs them and let go of after minutes unused, as the
  tags index's pieces are.
- ADR-003's status line names this record.
