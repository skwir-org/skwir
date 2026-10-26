# ADR-094: Vectors in two tiers, `about:` and `like:`, and the vector index

Status: accepted, 2026-10-26, the user's choice
(`docs/indexing-and-ai.md`). Not built.
Amends ADR-026 (hints gain a proof and a score) and takes Phase 7's
`semantic` and `models-small` items.

## Context

- Phase 7 planned a scoped, on-demand, incremental subtree index in the
  cache, natural language in the address bar, and small models doing
  the volume on CPU or iGPU while a language model orchestrates.
- The reference home holds 5.8 M entries; even the XDG folders of a
  photographer or a researcher hold hundreds of thousands of files. A
  transformer embedding costs tens of milliseconds per chunk on CPU; a
  static embedding (model2vec's layout: a token table and a matrix,
  mean-pooled) costs microseconds and 8 to 130 MB on disk, at a known
  loss in quality that a dense rerank of the few candidates recovers.
- The thumbnail service already decodes every picture shown at a bounded
  size; an image–text model's image tower embeds that, and its text
  tower embeds a query or a list of labels, so text-to-image search and
  zero-shot labels need no captioning.
- `hints::HintProvider` names candidates; a search checks each like any
  file. Meaning has no walk to fall back on: an answer by meaning is
  only as good as the index, and must say so.
- The user's files are French and English at least.

## Decision

1. **Two tiers.** The static model embeds every name, path and text
   chunk in scope, in Skwir's process, the matrix memory-mapped, the
   tokenizer the `tokenizers` crate's. The dense model, through `Models`
   (`Task::TextEmbedding`), embeds the query and reranks the top
   candidates; its vectors are kept for what was reranked or looked at,
   so the dense tier is learned where it is used. Both tiers must be
   multilingual; `skwir-bench embed` decides the models and whether the
   static tier alone serves names.
2. **Chunks.** A name with its folders' names is a chunk; text is cut at
   about 256 tokens on sentence or paragraph boundaries with an overlap,
   keeping its byte range in `extract.text` for the snippet shown.
3. **Pictures** are embedded from the thumbnail by the image–text model's
   image tower in the helper (ADR-093); `semantic.labels` is a field of
   zero-shot labels with confidences, provenance `Model`.
4. **The index,** per root and model (`semantic/<root>/<model-id>/`):
   segments of a file table, a chunk table and int8 vectors with a scale
   each, tombstones, the model's id, version and dimension in the
   header, so another model's vectors never pass for these; added and
   merged, never rewritten whole, rebuilt when damaged. Search is a
   brute-force dot product over memory-mapped int8 with SIMD and a
   top-k heap; an approximate index (usearch) is decided when a root
   passes a million vectors, by measurement, not before.
5. **Terms and chips.** The query plugin owns `about:"text"` (meaning:
   text and names; pictures through the text tower) and `like:"path"`
   (a file's own vectors: meaning for text, looks for a picture), and
   Find Similar gains "by meaning" and "by looks". They are answered
   through `hints::scored` with a proof (`hints::coverage`); the search
   orders by `keys::SCORE`, which is never written to a store. Its
   footer says "by meaning, as of 14:02, 1,204 files not yet read";
   under a root not covered it lists what the index has, says what it
   leaves out and links to `extract.build`. A search by meaning never
   walks.
6. **Room and lifetime:** `semantic.mb`; segments memory-mapped when a
   search needs them, let go of after minutes unused; the static matrix
   likewise.

## Consequences

- ADR-026's hints gain `coverage` (ADR-050's step A) and `scored`;
  `keys::SCORE` joins the core's keys; ADR-026's status line names this
  record.
- The `tokenizers` crate enters the `semantic` plugin; its licence goes
  through `just deny` and the notices.
- Zero-shot labels are the first picture suggestions (ADR-095) and need
  no language model.
- Budgets `static_embed_us` and `vector_search_us`; the bench's bilingual
  synthetic set keeps no real name.
- Natural language in the address (`query.ask`) builds on `about:` and
  is ADR-095's.
