# ADR-091: Derived facts are fields asked ahead of time: the extract provider

Status: accepted, 2026-10-26, the user's choice
(`docs/indexing-and-ai.md`). Not built.
Builds on ADR-023 (fields read on demand) and ADR-009's bounds; ADR-023's
status line names it.

## Context

- ADR-023 reads a costly fact only when something asks, keeps the answer
  by file identity and version in memory and in the cache, and never
  per row or per frame. The first field was a picture's position.
- An index of content needs the same facts for every file in scope, in
  bulk: text, a hash, a perceptual hash, OCR, a transcript. Phase 7's
  plan named an `extract` provider "cached by content hash, using
  installed tools where present", shared by search, previews and later
  embeddings.
- The previews already read every one of these: the reader helpers give
  a page's text, the documents plugin reads Word and OpenDocument or
  asks Pandoc, EPUB and email are read for their structure, the
  thumbnail service decodes every picture shown. Reading them a second
  way would be a second set of bugs and bounds.
- A copy, a move across volumes or a rename gives a file a new identity
  or version; its text is the same.

## Decision

1. **The `extract` plugin is a `FieldProvider`** for `extract.text`
   (bounded, with its parts' offsets), `extract.hash` (BLAKE3),
   `extract.phash` (a 64-bit perceptual hash), `extract.ocr`,
   `extract.transcript` and `extract.facts` (EXIF, media facts, page
   counts). The harvest (ADR-089) asks it through `Fields::get_all` for
   every file in scope, so an index is fields asked ahead of time; on
   demand, the same answers serve Properties, a search's last term and
   agents (`extract.text {entries}`, bounded and redacted as any answer).
2. **One read per file per version, by the previews' readers.** A text
   file is read as `content:` reads it; a document as its preview reads
   it (`preview::is_office`, `shown_as_pages`, the reader helpers,
   Pandoc), under the previews' quick bounds; a picture through the
   thumbnail service, never the original; sound through the media helper
   decoded to PCM in a scratch file. What the index holds is what the
   preview shows. A special file, a link's own entry, a file past
   `extract.max_mb` or a private file is never read here.
3. **Two keys.** Answers are kept by identity and version as every field
   is; the `extract` namespace also maps a content hash to the version
   whose facts are kept (`by-hash/`), so a file met again under another
   identity reuses them. The hash is computed in the same read, only
   when the file was read whole.
4. **Learned from looking.** A preview that extracted text, or a
   thumbnail the service decoded, hands what it has to
   `Fields::teach(key, identity, value)`, a core seam that keeps the
   value as the registered provider's answer at its current version and
   drops it when no provider answers the key. The index grows from what
   the user looks at with background indexing off; plugins stay
   independent, talking through core-owned data.
5. **A failed read is retryable, never an empty answer** (ADR-023's
   rule, skwird's rule): counted as unread under its root with the
   reason, tried again at the next reconcile.
6. **OCR and speech are off by default**, each a setting
   (`extract.read_pictures`, `extract.transcribe`; the field keys stay
   `extract.ocr` and `extract.transcript`), answered through `Models`
   (ADR-093) when on; the system's own OCR and speech first.

## Consequences

- `content:` can match documents (ADR-090); vectors have text to embed
  (ADR-094); cleaners have hashes (ADR-095); Properties gains "In the
  index: read 14:02, 1,230 words, 12 pages".
- The fields' on-disk room becomes a cap per provider; `extract.mb` is
  this provider's, and over it the provider answers on demand but the
  harvest stops learning (ADR-092).
- A new derived fact is a new key on the provider, no core change; a
  reader fixed for the preview is fixed for the index.
- `Fields::teach` is a new core seam; its test is a preview's text found
  in the cache by the harvest without a read.
- Budget `extract_text_us` (a thousand text files through
  `Fields::get_all`), and PDFs made by the bench for the documents'
  path.
