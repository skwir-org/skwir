# Optional previews

Open the preview panel with **F11**, or press **Space** for peek. New formats use
the shared toolbar, whose less frequent actions move into **More** in a narrow
panel. **Alt+Page Up / Page Down** changes sections, chapters or comic pages.

A file inside an archive, or attached to a message, opens as what it is: a
picture zooms, a PDF opens in the reader, JSON folds, Markdown reads. The name
in the header becomes a trail (`bundle.zip › notes.md`); click a step, or Back,
to return. The panel follows an archive inside an archive, and no deeper.

The header shows the entry's icon, name, type, size and date. Click it, or
press Ctrl+I, for **Details**, which opens under it and lists the rest of
what is known: exact size, folder, owner and access, times, tags, and what the format
says (a book's author, a font's family, an archive's unpacked size, a
picture's dimensions). It fills the panel when there is nothing to preview.
Click a value to copy it. The panel closes with the Preview button (F11).

Each format is enabled by an independent plugin in **Settings → Plugins**:

| Plugin | Formats and controls |
| --- | --- |
| `preview-audio` | Audio playback, seeking, volume, speed (pitch kept), tags, embedded cover and a spectrum ring in the cover's colour. Playback starts paused. Previous and Next step through the folder's audio files or a playlist; a track that ends hands over to the next. **Keep playing** pins the player so the music goes on while you browse, steered from the status bar and from the desktop's media keys (MPRIS), without a gap between tracks. ReplayGain is a setting. |
| `preview-hex` | Read-only hexadecimal/ASCII ranges for binary files; available explicitly for any file. Jump to a decimal or `0x` offset; search text or `hex: 00 ff 7a` byte sequences. |
| `preview-markdown` | Markdown and HTML reading mode with source toggle, headings, emphasis, lists, tables, code highlighting, links and local images. HTML keeps what its stylesheets say about emphasis, alignment, indents and hidden text, and is inert: no scripts, fonts or remote content. |
| `preview-archives` | File-list-style archive tree with themed icons, folders first, aligned sizes, expand/collapse controls and selected-member image/text previews; other members show paged hexadecimal bytes. |
| `preview-epub` | Reflowing EPUB chapters, contents, embedded images, links and footnotes (which open under their reference), text size, selection/copy, source and book search. The book's stylesheets give it its italics, centred titles, indents and hidden text; its fonts and colours give way to the theme. Chapters follow each other: scroll on past the end of one to read the next. A fixed-layout book of pictures (a comic) shows a page at a time. |
| `preview-playlists` | M3U/M3U8, PLS, XSPF and CUE files as a list of tracks with titles, artists and lengths; missing and remote entries are marked. A track on this machine opens in the player, whose Previous and Next then walk the playlist. Nothing is fetched. |
| `preview-tables` | CSV/TSV tables, quoted multiline cells, horizontal scrolling and section navigation. |
| `preview-structured` | Syntax-highlighted JSON/YAML/TOML code with line numbers, folding and source toggle. |
| `preview-fonts` | TTF/OTF/TTC specimens and character sheets. Find sets the specimen text; sections advance through supported characters. |
| `preview-email` | EML headers, inert HTML/text, embedded content-ID images and attachment previews. |
| `preview-comics` | CBZ/CBR/CB7/CBT image pages in numeric filename order. |

Text can be drag-selected and copied with the toolbar or **Ctrl+C** while the
reader has focus. With no text range, Copy uses the selected row or current
section. **Ctrl+F** searches the focused viewer; the search button is always
available. Repeating a search continues after the previous match: hex search
streams from the byte after the last hit; EPUB search first looks further in
the shown chapter, then asks the book for the next chapter with a match and
wraps around. Other viewers search the loaded section after the selected item.
The command palette exposes
Preview: hexadecimal, default presentation, source / rendered, find content,
headings and contents, copy content and page navigation.

Hex uses compact rows with byte-position headers, groups of eight bytes and
separate offset/ASCII columns. Zero bytes and non-printing placeholders are
muted; printable ASCII is accented. Font ligatures are disabled so adjacent
characters remain distinct.

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

ASF containers (`application/vnd.ms-asf`, `application/x-ms-asf`, or `.asf`)
use `preview-media`. GStreamer discovers their streams: video files use the video
player, while audio-only files show the audio presentation and spectrum. ASF needs
the [GStreamer ASF demuxer](https://gstreamer.freedesktop.org/documentation/asf/asfdemux.html)
from `gst-plugins-ugly`; the dev shell includes it alongside `gst-libav` for
Windows Media codecs. Reload the dev shell after changing its plugin packages.

EPUB is a native **reading mode**, not a browser: a book's stylesheets are
applied only where they say how text reads (emphasis, alignment, indents, relative
sizes, hidden text); its fonts, colours, page layout, scripts and DRM are not.
Footnotes are gathered at the end of their chapter; clicking a note reference
opens the note under it, from this chapter or another, and **Go to note**
leads there. Superscripts, subscripts and small capitals are drawn as such.
Pictures are decoded as they scroll into view. Markdown and email do not fetch remote images. Encrypted archives need
an external application. Selected container
members and mail attachments currently preview as images, text or bytes; they do
not launch nested PDF/video sessions. Large inputs have explicit bounds and
errors rather than unbounded parsing; see [ADR-009](adr/009-structured-previews.md).

Structured previews format JSON with two-space indentation; TOML and YAML keep
their original syntax, indentation, comments and ordering. Click a gutter arrow
to fold an object, array or section; the toolbar also offers expand/collapse all.
Find reveals matching lines, and text selection/copy preserves blank lines.
Source shows the original JSON formatting. These are read-only presentations.
Invalid or oversized documents fall back to paginated source with a diagnostic;
code previews are bounded to 2 MiB, 10000 lines and 4096 bytes per line.

## Reproducible checks

`just previews-check` generates temporary audio and archives and exercises real
workers, using a clocked fake audio sink. It verifies tags, spectrum, seek/pause,
archive member reads, path handling, caller-provided output limits and symlink
rejection. The archive worker takes every bound as an argument (`--members`,
`--names`, `--bytes`) from `bibli_platform::inspection::limits`. This
does not establish audible output quality on a real device.
It also generates ASF video and audio-only fixtures and checks stream detection,
playback, seeking and pause without an audio-only filename hint.

For manual UI checks, generate synthetic files outside the repository:

```sh
python3 tools/previews/check.py --root /tmp/bibli-preview-demo --generate-only
just run /tmp/bibli-preview-demo
```

The generator can copy an installed system font for the font specimen; generated
media and screenshots are not repository fixtures. Parser and plugin lifecycle
regressions also run in the normal workspace suite.
