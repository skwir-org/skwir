# Optional previews

Open the preview panel with **F3**, or press **Space** for peek. New formats use
the shared toolbar, whose less frequent actions move into **More** in a narrow
panel. **Alt+Page Up / Page Down** changes sections, chapters or comic pages.

Each format is enabled by an independent plugin in **Settings → Plugins**:

| Plugin | Formats and controls |
| --- | --- |
| `preview-audio` | Audio playback, seeking, volume, speed, tags, embedded cover and live spectrum. Playback starts paused. |
| `preview-hex` | Read-only hexadecimal/ASCII ranges for binary files; available explicitly for any file. Jump to a decimal or `0x` offset; search text or `hex: 00 ff 7a` byte sequences. |
| `preview-markdown` | Markdown reading mode with source toggle, headings, emphasis, lists, tables, code highlighting, links and local images. |
| `preview-archives` | Archive tree, folder collapse and selected-member image/text/byte previews. |
| `preview-epub` | Reflowing EPUB chapters, contents, embedded images, links, text size, selection/copy, source and book search. |
| `preview-tables` | CSV/TSV tables, quoted multiline cells, horizontal scrolling and section navigation. |
| `preview-structured` | Expandable JSON/YAML/TOML trees and source toggle. |
| `preview-fonts` | TTF/OTF/TTC specimens and character sheets. Find sets the specimen text; sections advance through supported characters. |
| `preview-email` | EML headers, inert HTML/text, embedded content-ID images and attachment previews. |
| `preview-comics` | CBZ/CBR/CB7/CBT image pages in numeric filename order. |

Text can be drag-selected and copied with the toolbar or **Ctrl+C** while the
reader has focus. With no text range, Copy uses the selected row or current
section. **Ctrl+F** searches the focused viewer; the search button is always
available. Hex search streams from the current offset; EPUB search checks book
chapters. Other viewers search the loaded section. The command palette exposes
Preview: hexadecimal, default presentation, source / rendered, find content,
headings and contents, copy content and page navigation.

## Dependencies

`just run` builds the native helpers alongside bibli. The Nix shell includes the
GStreamer and libarchive development/runtime dependencies. Packagers can omit
helpers; their plugins explain what is missing when selected. Pure Rust formats
need no external runtime or network service.

- Audio: `bibli-media-worker` plus GStreamer playback/codec plugins; the spectrum
  element comes from the good plugins. FFmpeg/ffprobe provide optional cover art.
- Archives and comics: `bibli-archive-worker` plus libarchive. Neither 7-Zip nor
  libarchive loads into the UI process.
- Existing PDF, Office and video backend settings are unchanged.

EPUB is a native **reading mode**, not a browser: publisher CSS, fixed-page
layouts, scripts and DRM are unsupported. Markdown and email do not fetch remote
images. Encrypted archives need an external application. Selected container
members and mail attachments currently preview as images, text or bytes; they do
not launch nested PDF/video sessions. Large inputs have explicit bounds and
errors rather than unbounded parsing; see [ADR-009](adr/009-structured-previews.md).

## Reproducible checks

`just previews-check` generates temporary audio and archives and exercises real
workers, using a clocked fake audio sink. It verifies tags, spectrum, seek/pause,
archive member reads, path handling, output limits and symlink rejection. This
does not establish audible output quality on a real device.

For manual UI checks, generate synthetic files outside the repository:

```sh
python3 tools/previews/check.py --root /tmp/bibli-preview-demo --generate-only
just run /tmp/bibli-preview-demo
```

The generator can copy an installed system font for the font specimen; generated
media and screenshots are not repository fixtures. Parser and plugin lifecycle
regressions also run in the normal workspace suite.
