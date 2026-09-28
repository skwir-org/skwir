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
| `preview-media` | Audio and video in one player: playback, seeking, volume, speed (pitch kept), audio and subtitle tracks, fullscreen for video; tags, embedded cover and a spectrum ring in the cover's colour for audio. Previous and Next step through the folder's files of the same kind or a playlist. **Keep playing while you browse** (on by default) pins what plays so it goes on in a player at the foot of the preview, steered from the status bar and from the desktop's media keys (MPRIS), without a gap between tracks. **Play on selection** starts a file once selected; **When a track ends** stops, plays the next or shuffles. ReplayGain and the video engine are settings. |
| `preview-hex` | Read-only hexadecimal/ASCII ranges for binary files; available explicitly for any file. Jump to a decimal or `0x` offset; search text or `hex: 00 ff 7a` byte sequences. |
| `preview-markdown` | Markdown and HTML reading mode with source toggle, headings, emphasis, lists, tables, code highlighting, links and local images. HTML keeps what its stylesheets say about emphasis, alignment, indents and hidden text, and is inert: no scripts, fonts or remote content. Fenced `mermaid` blocks are drawn as diagrams when `bibli-mermaid-worker` is installed, and stay code without it. Jupyter notebooks (`.ipynb`, format 4) read the same way: Markdown cells, code in the kernel's language, and what each cell printed, returned, raised or plotted. |
| `preview-archives` | File-list-style archive tree with themed icons, folders first, aligned sizes, expand/collapse controls and selected-member image/text previews; other members show paged hexadecimal bytes. |
| `preview-epub` | Reflowing EPUB chapters, contents, embedded images, links and footnotes (which open under their reference), text size, selection/copy, source and book search. The book's stylesheets give it its italics, centred titles, indents and hidden text; its fonts and colours give way to the theme. Chapters follow each other: scroll on past the end of one to read the next. A fixed-layout book (a comic, a picture book) opens in the document reader like a PDF, continuously or a page at a time, each page drawn as its book lays it out, lettering in the book's own fonts, and sharper as it is zoomed. The Book engine setting can instead give every book to MuPDF (optional worker), which lays it out into pages in the document reader; there the size keys lay it out again at a larger or smaller text size and keep the place. How wide lines may grow is the Reading width preference, or the Full width button beside the text size. The toolbar reads like the document reader's: previous chapter, the chapter number (click to type one), next; smaller, the text size as a picker, larger; then Full width. A book that lists the page numbers of its printed edition can be counted by them instead: the picker after the count switches between chapters and printed pages, and the Printed page numbers preference remembers the choice. A PDF that names its pages (front matter in roman numerals, a body starting at 1 further in) is counted the same way in the reader. |
| `preview-playlists` | M3U/M3U8, PLS, XSPF and CUE files as a list of tracks with titles, artists and lengths; missing and remote entries are marked. A track on this machine opens in the player, whose Previous and Next then walk the playlist. Nothing is fetched. |
| `preview-tables` | CSV/TSV tables, quoted multiline cells, horizontal scrolling and section navigation. JSON Lines (`.jsonl`, `.ndjson`): a row per line, the keys as columns. Excel workbooks (`.xlsx`, `.xlsm`, `.xlsb`): each sheet's cells, sheet after sheet; the **Spreadsheets** setting (Table or Pages) gives them to LibreOffice's pages instead. SQLite databases: each table's rows, table after table, opened read-only so nothing is written beside them. |
| `preview-structured` | Syntax-highlighted JSON/YAML/TOML code with line numbers, folding and source toggle. JSON with comments (JSONC, `tsconfig.json`) and JSON5 are shown as written. |
| `preview-fonts` | TTF/OTF/TTC and WOFF/WOFF2 specimens and character sheets. Find sets the specimen text; sections advance through supported characters. |
| `preview-models` | 3D models, drawings and prints shown by the picture their application stored in them, in the panel and the grid: Blender files (and their `.blend1` backups, compressed or not), 3MF packages (the package thumbnail, or Bambu Studio's and Orca's first plate), FreeCAD documents, SketchUp models and LayOut documents, Rhino models, DWG drawings, and G-code, as text or Prusa's binary `.bgcode`. DXF drawings are drawn from what they hold, as a CAD program shows model space in AutoCAD's colours: lines, arcs, circles, polylines, ellipses, splines, solids, hatches, text and inserted blocks (dimensions among them), with their size in the drawing's unit, entities and blocks in Details (a DXF of more than 32 MiB shows the picture it stored, if any). Meshes that store no picture (STL, OBJ, PLY, OFF, glTF and GLB, a 3MF its slicer did not picture) are drawn by `bibli-model-worker`, seen from the front, right and above, and measured: size, volume and whether they are watertight, triangles and objects (an STL is read in millimetres, as slicers read it). Details lists what the file says: the Blender version and scenes, the SketchUp or Rhino version and a model's scenes or a LayOut document's pages, a print's time, filament, material, layer height and printer, the slicer, a drawing's DWG or DXF version. G-code that stores no picture is drawn from its moves: a print as a slicer previews it (extrusions from the front, right and above, in PrusaSlicer's colours for the features its slicer marks, or by height), a milling, engraving or laser program (`.nc`, `.ngc`, `.tap`, `.cnc` too) from above, its cuts coloured by depth and its rapid moves faint; Details adds the size of the work. A program that computes its moves (LinuxCNC's parameters, subroutines) reads as text. G-code keeps its text one toggle away. |
| `preview-circuits` | KiCad boards (`.kicad_pcb`) drawn as pcbnew shows them: both sides' copper with filled zones, pads, vias and holes, silkscreens and the outline; footprints (`.kicad_mod`) alone; schematics (`.kicad_sch`) as eeschema draws them: symbols with their pins and fields, wires, buses, junctions, labels, notes and sheets; a symbol library's (`.kicad_sym`) first symbol. Details lists the KiCad version, title block, board size, copper layers, thickness, footprints, pads, nets, tracks and vias, or a schematic's paper, symbols and sheets. The text is one toggle away. Text is set in a system font (KiCad's own is not carried), so it is close to KiCad's but not identical. Gerber layers (`.gbr`, `.gtl`, `.gbl`, `.gts`, `.gto`, `.gm1`, `.g2`… from any design program) are drawn one at a time in the colour KiCad gives their layer, with their macros, regions, arcs, clear polarity, step and repeat and aperture blocks; Details names the layer (from the file's X2 attributes, else its name), the program that made it and its size. Excellon drill files (`.drl`, `.xln`, `.exc`) show their holes, slots and routes, with the holes, tools and sizes in Details. A `.gbr` that is a GIMP brush is left to the image previews. Files of more than 32 MiB are not drawn. |
| `preview-email` | EML headers, inert HTML/text, embedded content-ID images and attachment previews. |
| `preview-comics` | CBZ/CBR/CB7/CBT comics in the document reader, continuously or a page at a time, pages in numeric filename order, each decoded at the size it is shown. |

Pictures are decoded in the process (PNG, JPEG, GIF, WebP, BMP, TIFF, ICO,
QOI, TGA, PNM, OpenEXR, DDS) and SVG drawings are drawn, sharper as they are
zoomed. A picture of another kind (HEIC, AVIF, JPEG XL, a camera's raw file)
is shown as the desktop's thumbnailer for it draws it, 1024 pixels at most,
when one is installed (`thumbnails` plugin). So is a 3D model (STL, glTF,
STEP, IGES, a Blender file…) when f3d, Blender or another thumbnailer
claims its type. Code is highlighted with bat's
grammars.

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
- SQLite databases: the system's `libsqlite3`, which bibli links.
- Mermaid diagrams: `bibli-mermaid-worker` (pure Rust, built by default, kept
  out of bibli because its layout engine is about 16 MB).
- 3D meshes without a stored picture: `bibli-model-worker` (pure Rust, built
  by default). Without it, an installed thumbnailer (f3d) draws them, if any.
- HEIC, AVIF, JPEG XL and raw photos: a thumbnailer for them (libheif's
  `heif-thumbnailer`, for example); without one they show no picture.

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
Pictures are decoded as they scroll into view. A fixed-layout page that
states its size is drawn as its book lays it out (text over pictures, in the
book's fonts); floats, tables and columns on such pages come out
approximately, and a drawn page has no selectable text: search in such a
book finds the pages, without marking the words. Markdown and email do not fetch remote images. Encrypted archives need
an external application. Selected container
members and mail attachments currently preview as images, text or bytes; they do
not launch nested PDF/video sessions. Large inputs have explicit bounds and
errors rather than unbounded parsing; see [ADR-009](adr/009-structured-previews.md).

Structured previews format JSON with two-space indentation; TOML and YAML keep
their original syntax, indentation, comments and ordering. Click a gutter arrow
to fold an object, array or section; the toolbar also offers expand/collapse all.
Find reveals matching lines, and text selection/copy preserves blank lines.
Source shows the original JSON formatting. JSON with comments or trailing
commas, JSONC and JSON5 are never reformatted: they show as written, folded
where their brackets say. These are read-only presentations.
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
