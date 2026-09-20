# Document reader invariants

Read this before touching `crates/pdf-worker`, `crates/poppler-worker`, `crates/platform/src/document.rs`, `crates/ui/src/reader*.rs` or `crates/ui/src/elements/document.rs`. It is binding, like `AGENTS.md`.

- Native PDF libraries load only in `bibli-pdf-worker` / `bibli-poppler-worker`,
  never in the UI process. Both engines use the same protocol/session manager.
  Session creation/render/search are blocking worker entry points. The manager
  thread owns child cleanup and temporary files; dropping a UI session must
  only cancel/send, never wait for a process or delete files.
- A session follows both the selected preview and its provider registration.
  Page/search requests have separate cancellation and generation checks.
  Cancel background search before a visible page request. Disabling a provider
  must cancel even an idle session; test this without requiring native libraries.
- Document pages transfer one owned pixel buffer into GPUI; swizzle it in
  place. Full-page copies on scheduler threads can retain large allocations
  across allocator arenas even after the preview is closed.
- `RenderImage` reference counts do not release uploaded GPUI atlas entries.
  Explicitly call `cx.drop_image(image, Some(window))` when replacing/closing
  a preview, as the thumbnail LRU does on eviction.
- Continuous reading virtualizes pages with a logical page/offset anchor. Size
  changes and fit/zoom are resolved in prepaint; invalidate measured row heights
  without resetting that anchor. Render only demanded pages and neighbours,
  serialize requests, reject cancelled generations, and evict offscreen pixel
  buffers and their atlas entries together. Page interactions carry a page
  number; never apply an adjacent page's selection or links to the opening page.
  Overdraw measurement is not stable between frames: keep prefetch neighbours
  explicit so a later layout does not cancel the work it just requested.
- PDF pixel, text, link and search rectangles share normalized coordinates.
  Fit/zoom transforms belong in `elements/document.rs` prepaint. Verify rotated
  pages with `tools/pdf/check.py` for both engines; use only its synthetic
  fixture generator. Poppler text layout has a top-left origin, whereas its
  search/link rectangles have a bottom-left origin; normalize at the worker.
- `auto` may fall back when the worker/library is absent. Report document
  errors directly; do not conceal malformed files by silently trying engines.
  Poppler provides the same reader features. CLI image fallback is for `auto`
  only when neither reader worker is available. Measurements must distinguish
  renderer time from full UI latency and include child PSS, not only bibli PSS.
