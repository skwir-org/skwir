# Structured preview invariants

Read this before touching `crates/core/src/inspection.rs`, `crates/platform/src/{archive,inspection,rich_preview}.rs`, `crates/archive-worker`, any `crates/plugins/preview-*` crate or `crates/ui/src/inspector*.rs` / `code_preview.rs`. It is binding, like `AGENTS.md`.

- New format plugins publish `inspection::Inspection` from scheduler workers;
  use the existing preview registry and owner lifetimes. Fallback providers have
  lower priority than typed providers, including after disable/re-enable.
- Keep rich text, table/tree rows and images toolkit-independent. The UI owns
  virtualized layout and commands. Bound input, expansion and visible work;
  a short file can still contain one huge field or deeply nested structure.
- HTML reading mode is inert: no scripts, remote assets or automatic link opens.
  Markdown images must stay within the document directory after canonicalizing;
  EPUB resources stay inside their container and email images use content IDs.
- Only `bibli-archive-worker` links libarchive. Member names are labels/keys,
  never extraction paths. Read only selected regular members, bound the actual
  decompressed bytes and keep temporary cleanup off the UI thread.

## Structured code previews

- Code inspections contain one bounded logical line per block. Fold ranges refer
  to those block indices; do not pass them through `split_blocks` or change the
  line order after deriving folds. Ranges must be nested or disjoint.
- Derive folds from parser spans/events, not indentation or delimiters inside
  strings. Preserve TOML/YAML comments and syntax. Highlight complete documents
  on a worker with parser state retained between lines; rendering only consumes
  prepared runs and visible indices. Copy must retain empty selected lines.
