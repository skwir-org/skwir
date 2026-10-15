# Previews

What Skwir previews and how, kind by kind; then what each preview needs
installed (for packagers) and how previews are checked (for
contributors). The rules for working on preview code are in
[`invariants/previews.md`](invariants/previews.md) and
[`invariants/media.md`](invariants/media.md).

## Using the preview

- `Space` peeks at a file. `F11` (or `Ctrl+Shift+Space`) opens the
  preview panel, which follows the selection; `F11` or the Preview button
  closes it.
- **The header** shows the file's icon, name, type, size and date. Click
  it, or press `Ctrl+I`, for **Properties**: exact size, folder, owner and
  access, times, tags, and what the format says (a book's author, a
  font's family, an archive's unpacked size, a picture's dimensions).
  Properties fills the panel when there is nothing to preview. Click a value
  to copy it.
- **The bar** holds the tools of the kind shown; in a narrow panel the
  less used ones move into More. Every tool is also a palette command
  (`Preview: …`, `Player: …`).
- **Fullscreen.** A picture, a document's pages, text, a film or music
  goes on the whole screen with the bar's Fullscreen button, `F`, or a
  double click on a picture or a film; `F` or `Escape` brings it back.
  Above pages or text the bar stays at the top, what is shown under it;
  over a picture, a film or music it hides when the pointer rests, and
  so does the pointer (move it to bring both back). `C` hides or shows
  the bar. The arrows go to the picture before or after in the folder
  (other files are skipped), or turn the page.
  **Slideshow** (`Space` on the whole screen, or More › Slideshow in the
  panel) shows the pictures from the one shown to the last, each for the
  time its bar's picker says (Reading › Pictures). The next picture is
  read while you look at one, so it shows at once. The screen stays awake
  while a slideshow or a film plays.
- **Files inside an archive or a message** open as what they are: a
  picture zooms, a PDF opens in the reader, JSON folds, Markdown reads.
  The file opens below or beside the archive's list (More › Layout), and
  the header becomes a trail (`bundle.zip › notes.md`): click a step, or
  press `Escape` or ×, to return. The panel follows an archive inside an
  archive, no deeper.
- **Links.** A link in a document opens with a click; an address found
  in plain text opens with `Ctrl+click`. A link to a file or folder shows
  it in Skwir; a web address goes to your browser.
- **Select and copy.** Drag to select text; copy with `Ctrl+C` or the
  bar. With no text selected, Copy takes the selected row or the current
  section.
- **Source.** A rendered file (Markdown, HTML, a table, a message, a
  book's chapter, a track, an SVG drawing) shows its own text with the
  bar's Source button, lit while on. It stays on for every file of that
  type, also after a restart, until you turn it off; other types keep
  their presentation. A file of the type that holds no text opens as
  usual.
- **When a preview fails.** A text file that cannot be presented (too
  large to lay out, invalid, a drawing that does not draw) shows its
  text instead, saying why ("Shown as written: …"). Any other file says
  why it is not shown, with **View as hex** in the bar, the one way left
  to look at it; elsewhere View as hex is in More.
- **Find.** `Ctrl+F` searches the preview that has the keyboard; the
  search button is always there. Searching again continues after the
  last match: a text file is searched whole, either way and around its
  end; hex search streams on from the byte after it; a book searches the
  rest of the chapter and then the next chapter with a match, wrapping
  around; other previews search the loaded section after the selected
  item.
- Each kind is a plugin, turned on or off in Settings › Plugins.

### Keys

| Keys | Where | Action |
| --- | --- | --- |
| Arrows | Peek | Previous or next file |
| `Space`, `Escape` | Peek | Close |
| `Ctrl+I`, `Alt+Enter` | Anywhere | Properties |
| `Ctrl+F`; `F3`, `Shift+F3` | Preview | Find; next or previous match |
| `Ctrl+C`, `Ctrl+A` | Preview | Copy; select all |
| `Page Up`, `Page Down` | Reader | Previous or next page |
| `[`, `]` | Reader, book | Previous or next page or chapter |
| Up, Down; `Page Up`, `Page Down`; `Ctrl+Home`, `Ctrl+End` | Text | A line; a screen; the start or the end |
| `Alt+Z` | Text | Wrap long lines, or let them go on sideways |
| `Alt+Page Up`, `Alt+Page Down` | Any preview | Previous or next section, chapter or comic page |
| `F` | Picture, reader, text, peek | Fullscreen, or back |
| `Ctrl++`, `Ctrl+-`, `Ctrl+0` | Reader, picture, map, text | Zoom in, out, reset; text larger, smaller, as it was |
| `Ctrl` and the wheel | Over a page, a picture or text | Zoom in or out; text larger or smaller (a map zooms with the wheel alone) |
| Arrows | Map | Pan |
| Up, Down; Left, Right | Archive or message | Walk its files; fold or unfold |
| `Ctrl+Shift+O` | Text and structured previews | Label the links in view for the keyboard |

On the whole screen:

| Keys | Action |
| --- | --- |
| `Left`, `Right` | Previous or next picture, page or section |
| `Space` | Play or pause the slideshow (pictures) |
| `C` | Show or hide the bar |
| `F`, `Escape` | Back to the panel |

The player, in the panel or pinned at its foot:

| Keys | Action |
| --- | --- |
| `Space` | Play or pause |
| `Left`, `Right` | Seek 5 seconds (`Ctrl+Left`, `Ctrl+Right` in peek) |
| `Shift+Left`, `Shift+Right` | Previous or next track |
| `Up`, `Down` | Volume |
| `M` | Mute |
| `F` | Fullscreen |
| `Escape` | Leave fullscreen, else close the preview (in the panel) |

## By kind of file

### Text and code

- A text file of any size is shown whole: it opens at once, its lines
  numbered from the first, counted as you read (the bar says how many),
  and only what is on screen is read from the file. A huge log scrolls to
  its end, and Find and Copy see all of it (Copy up to 16 MiB). The
  source of a rendered file (Markdown, a table, a map) is read the same
  way.
- Long lines wrap to the panel, the line's number on its first row; More
  › Wrap lines (`Alt+Z`, Reading › Code) lets them go on sideways.
- Code is highlighted with bat's grammars, in Skwir's theme or one of
  bat's, themes you added to bat included (Reading › Code), light or
  dark whatever Skwir is. Colors follow what is read: after a jump into
  the middle of a long file, a string or comment open across the jump
  can be colored wrongly until it ends.
- The text is decoded once, from its first bytes: UTF-8, UTF-16 by its
  mark, else Windows-1252 (and the bar says so).
- Any file can be shown as bytes (see [Bytes](#bytes-hex)).

### Structured data (`structured-data`)

- JSON, YAML and TOML with syntax colors, line numbers and folding.
  Click a gutter arrow to fold an object, array or section; the bar
  expands or collapses all.
- JSON is formatted with two-space indentation, its keys in order;
  Source shows its original formatting. TOML and YAML are shown as
  written (syntax, indentation, comments and order), so they have no
  Source to switch to.
- JSON with comments or trailing commas (JSONC, `tsconfig.json`) and
  JSON5 are never reformatted: shown as written, folded where their
  brackets say.
- Find reveals matching lines; copying keeps blank lines. Read-only.

### Markdown, HTML and notebooks (`markdown`)

- Markdown and HTML in reading mode, with a source toggle: headings,
  emphasis, lists, tables, highlighted code, links and local images.
- HTML keeps what its stylesheets say about emphasis, alignment, indents
  and hidden text. It is inert: no scripts, fonts or remote content.
- Fenced `mermaid` blocks are drawn as diagrams when
  `skwir-mermaid-worker` is installed, and stay code without it.
- Jupyter notebooks (`.ipynb`, format 4): Markdown cells, code in the
  kernel's language, and what each cell printed, returned, raised or
  plotted.

### Tables (`tables`)

- CSV and TSV, quoted multi-line cells included, with horizontal
  scrolling and section navigation.
- JSON Lines (`.jsonl`, `.ndjson`): a row per line, the keys as columns.
- Excel workbooks (`.xlsx`, `.xlsm`, `.xlsb`): each sheet's cells, one
  sheet after another. The **Spreadsheets** setting (Table or Pages)
  can give them to LibreOffice's pages instead.
- SQLite databases: each table's rows, opened read-only so nothing is
  written beside them.

### Documents (`documents`)

- **PDF** in the document reader: continuous or a page at a time, zoom,
  search, contents, links, text selection and copy. **Document engine**:
  Automatic prefers PDFium and falls back to Poppler; MuPDF is used only
  when chosen. A PDF that names its pages (roman numerals for the front
  matter, a body starting at 1 further in) can be counted by those
  names, as a book by its printed pages (see [Books](#books-epub)).
- **Office documents** as pages, converted by LibreOffice in a sandbox.
- **Word** (`.docx`, `.docm`, `.dotx`, `.dotm`), **OpenDocument text**
  (`.odt`, `.ott`) and **RTF** follow the **Text documents** setting:
  - Reading (default): Skwir reads Word and OpenDocument files itself, at
    once and fitted to the panel: headings, numbered lists, tables,
    footnotes that open under their reference, links and pictures. RTF
    goes to Pandoc.
  - Pages: LibreOffice lays them out as it prints them (a few seconds
    each).
  - Pandoc: Pandoc reads them all.

  A choice whose program is missing gives way to another.
- A long document is read in parts; `[` and `]` move between them.
- The grid shows the picture of the first page that an Office or
  OpenDocument file stores, before any preview.

### Books (`epub`)

- EPUB in reading mode, not a browser: chapters reflow, with contents,
  embedded images, links, text size, selection and copy, source, and
  search through the book. Chapters follow each other: scroll past the
  end of one to read the next.
- A book's stylesheets apply only to how text reads (emphasis,
  alignment, indents, relative sizes, hidden text). Its fonts, colors,
  page layout, scripts and DRM do not; the theme wins. Superscripts,
  subscripts and small capitals are drawn as such. Pictures are decoded
  as they scroll into view.
- Footnotes are gathered at the end of their chapter. Clicking a note
  reference opens the note under it, from this chapter or another; **Go
  to note** leads there.
- The bar: previous chapter, the chapter number (click to type one),
  next; smaller, the text size, larger; **Full width**. How wide lines
  may grow is the **Reading width** preference.
- A book that lists its printed edition's page numbers can be counted by
  them: the picker after the count switches between chapters and printed
  pages, and the **Printed page numbers** preference remembers it.
- A fixed-layout book (a comic, a picture book) opens in the document
  reader like a PDF: each page drawn as the book lays it out, lettering
  in the book's fonts, sharper as it is zoomed. Floats, tables and
  columns on such pages are approximate, and a drawn page has no
  selectable text: search finds the pages without marking the words.
- **Book engine**: MuPDF (optional helper) can lay every book out into
  pages in the document reader instead; the size keys then lay it out
  again at a larger or smaller text size, keeping the place.

### Comics (`comics`)

- CBZ, CBR, CB7 and CBT in the document reader, continuously or a page
  at a time. Pages are in numeric file-name order, each decoded at the
  size shown.

### DjVu (`djvu`)

- **DjVu** documents (`.djvu`, `.djv`), scanned books mostly, in the
  document reader: continuous or a page at a time, zoom, search, contents,
  links, text selection and copy, where the document has a text layer.
  The grid shows the first page.
- **DjVu engine**: Automatic prefers DjVuLibre, where its helper and
  library are installed, and falls back to djvu-rs, which every package
  carries; or choose one. DjVuLibre's pages are a little smoother when
  shrunk, and it reads the names a book gives its pages (`ii`).
- On the whole screen the arrows and the slideshow walk past DjVu files,
  as past other documents: only its bytes say whether one is a page or a
  book.
- Not read: a document whose pages are files of their own (an
  "indirect" DjVu), annotations other than links.

### E-mail (`email`)

- EML messages: headers, the body as inert HTML or text, embedded
  pictures, and attachments, each previewed as the file it is. Remote
  content is never fetched.

### Pictures

- Decoded in Skwir: PNG, JPEG, GIF, WebP, BMP, TIFF, ICO, QOI, TGA, PNM,
  OpenEXR, DDS. SVG drawings (`.svg`, `.svgz`) are drawn, thumbnails
  included, and sharper as the preview is zoomed.
- Other kinds (HEIC, AVIF, JPEG XL, a camera's raw file) are shown as
  the desktop's thumbnailer for them draws them, at most 1024 pixels,
  when one is installed (`thumbnails` plugin). Otherwise ImageMagick
  draws them (**Use ImageMagick**, on by default): AVIF, HEIC, JPEG XL,
  JPEG 2000, raw files, Photoshop and GIMP images, whatever its build
  reads (DjVu too, when the `djvu` plugin is off).
- Thumbnailers and ImageMagick run isolated from your files and the
  network where bubblewrap works
  ([ADR-031](adr/031-pictures-by-imagemagick-thumbnailers-isolated.md)).

### Audio and video (`media`)

- One player for both: play, seek, volume, speed (pitch kept), audio and
  subtitle tracks, fullscreen for video. Audio shows its tags, its
  embedded cover and a spectrum ring in the cover's color.
- Previous and Next step through the folder's files of the same kind, or
  a playlist.
- **Keep playing while you browse** (on by default) pins what plays to a
  player at the foot of the preview panel, with no gap between tracks.
  The last pane's footer steers it, and so do the desktop's media keys
  (`mpris`, Media controls).
- **Play on selection** starts a file once it is selected. **When a
  track ends**: stop, play the next, or shuffle. **Even out loudness**
  uses ReplayGain. **Video engine**: Automatic plays with GStreamer and
  falls back to a still frame from FFmpeg.
- ASF files (`.asf`, Windows Media): GStreamer finds their streams; a
  video plays in the video player, audio alone shows the audio
  presentation and spectrum.

### Playlists (`playlists`)

- M3U and M3U8, PLS, XSPF and CUE as a list of tracks with titles,
  artists and lengths; missing and remote entries are marked.
- A track on this machine opens in the player, whose Previous and Next
  then walk the playlist. Nothing is fetched.

### Archives (`archives`)

- ZIP, tar, 7z and other archives as a tree: folders first, icons,
  aligned sizes, expand and collapse. A file selected in it opens as what
  it is (see [Using the preview](#using-the-preview)).
- **Extract Here**, in an archive's menu or a drop's, extracts it beside
  it: an archive of one folder or one file gives that folder or file,
  one of several a folder named after it. With two panes, `Shift+F7`
  offers it into the other pane's folder.
- An encrypted ZIP asks its password. libarchive cannot decrypt an
  encrypted 7z or RAR: those need another application.
- A name already there asks, as a copy does: keep both (`photos (2)`),
  replace, or skip.
- **Compress to ZIP**, in the menu of what you select, makes `photos.zip`
  beside it; **Compress…** asks the name, whose ending picks the format
  (`.zip`, `.7z`, `.tar.gz`, `.tar.xz`, `.tar.zst`, `.tar.bz2`, `.tar`);
  **Compress with a Password…** (under More) makes an encrypted ZIP.
  Tags kept on files travel in a tar, not in a ZIP or a 7z.

### Fonts (`fonts`)

- TTF, OTF, TTC, WOFF and WOFF2 as specimens and character sheets,
  without installing them. Find sets the specimen text; sections step
  through the supported characters.

### 3D models, drawings and prints (`models`)

- **Stored pictures**, in the panel and the grid: Blender files (and
  their `.blend1` backups, compressed or not), 3MF packages (the package
  thumbnail, or Bambu Studio's and Orca's first plate), FreeCAD
  documents, SketchUp models and LayOut documents, Rhino models, DWG
  drawings, and G-code (text or Prusa's binary `.bgcode`).
- **DXF** drawings are drawn from what they hold, as a CAD program shows
  model space, in AutoCAD's colors: lines, arcs, circles, polylines,
  ellipses, splines, solids, hatches, text and inserted blocks
  (dimensions among them). Properties gives the size in the drawing's unit,
  entities and blocks. A DXF over 32 MiB shows its stored picture, if
  any.
- **Meshes** that store no picture (STL, OBJ, PLY, OFF, glTF and GLB, a
  3MF its slicer did not picture) are drawn by `skwir-model-worker` from
  the front, right and above, and measured: size, volume, whether they
  are watertight, triangles and objects. An STL is read in millimetres,
  as slicers read it.
- **G-code** that stores no picture is drawn from its moves:
  - a print as a slicer previews it: extrusions from the front, right
    and above, in PrusaSlicer's colors for the features its slicer
    marks, or by height;
  - a milling, engraving or laser program (also `.nc`, `.ngc`, `.tap`,
    `.cnc`) from above, cuts colored by depth, rapid moves faint;
    Properties adds the size of the work;
  - a program that computes its moves (LinuxCNC's parameters,
    subroutines) reads as text. The text is always one toggle away.
- **Properties** lists what the file says: the Blender version and scenes,
  the SketchUp or Rhino version and a model's scenes or a LayOut
  document's pages, a print's time, filament, material, layer height,
  printer and slicer, a drawing's DWG or DXF version.
- Other models (STEP, IGES…) are shown when an installed thumbnailer
  (f3d, Blender) claims their type, isolated like picture thumbnailers.

### Circuit boards (`circuits`)

- **KiCad**: boards (`.kicad_pcb`) as pcbnew shows them (both sides'
  copper with filled zones, pads, vias and holes, silkscreens, outline);
  footprints (`.kicad_mod`); schematics (`.kicad_sch`) as eeschema draws
  them (symbols with pins and fields, wires, buses, junctions, labels,
  notes, sheets); a symbol library's (`.kicad_sym`) first symbol.
  Properties lists the KiCad version, title block, board size, copper
  layers, thickness, footprints, pads, nets, tracks and vias, or a
  schematic's paper, symbols and sheets. Text is set in a system font
  (KiCad's is not carried): close to KiCad's, not identical.
- **Gerber** layers (`.gbr`, `.gtl`, `.gbl`, `.gts`, `.gto`, `.gm1`,
  `.g2`…, from any design program), one at a time in the color KiCad
  gives their layer: macros, regions, arcs, clear polarity, step and
  repeat, aperture blocks. Properties names the layer (from the file's X2
  attributes, else its name), the program that made it and its size. A
  `.gbr` that is a GIMP brush goes to the picture previews.
- **Excellon** drill files (`.drl`, `.xln`, `.exc`): holes, slots and
  routes, with holes, tools and sizes in Properties.
- The text is one toggle away. Files over 32 MiB are not drawn.

### Maps and tracks (`geo`, `map-tiles`)

- GPS tracks and routes (GPX, KML, KMZ, GeoJSON, TCX, FIT, IGC, NMEA),
  flight plans (Garmin, Flight Simulator, X-Plane, Little Navmap) and
  drone missions (QGroundControl, MAVLink, Litchi, DJI) on a map, with
  their distance, time and climb. Nothing a file links to is fetched.
- Maps kept in files (PMTiles, MBTiles).
- The background is the outline of the land, drawn offline, until you
  choose OpenStreetMap or a tile server you add (Settings › Map
  backgrounds). `map-tiles` is the only plugin that uses the network.

### Bytes (`hex`)

- **View as hex**, for any file: read-only, compact rows with
  byte-position headers, groups of eight bytes, separate offset and
  ASCII columns. Zero bytes and non-printing placeholders are muted,
  printable ASCII accented; font ligatures are off so adjacent
  characters stay distinct.
- The whole file in one view, a row of 16 bytes for each, read as you
  scroll: the scrollbar, `Ctrl+Home` and `Ctrl+End` reach any byte.
- Jump to a decimal or `0x` offset; search text or bytes
  (`hex: 00 ff 7a`), forward from the row selected, again from after the
  match.

## Limits

Large inputs have explicit bounds and errors rather than unbounded
parsing ([ADR-009](adr/009-structured-previews.md)):

- Text parsed whole (Markdown, tables, structured code): 2 MiB. Code:
  10,000 lines and 4096 bytes per line. A document that is invalid or
  too large shows its text instead, read as you scroll, with the reason.
- DXF drawings and circuit files: 32 MiB.
- Containers: an archive inside an archive, no deeper.

## Dependencies (for packagers)

`just run` builds every helper alongside Skwir, and the Nix dev shell has
their libraries. A packager can leave out any helper: its plugin says
what is missing when a file needs it. Formats read in Rust need no
external program or network service.

| Preview | Helper | Needs at run time |
| --- | --- | --- |
| PDF | `skwir-pdf-worker` | PDFium (the packages carry it) |
| PDF through Poppler | `skwir-poppler-worker` | Poppler |
| PDF and books through MuPDF | `skwir-mupdf-worker` | MuPDF (linked) |
| DjVu | `skwir-djvu-worker` | Nothing (djvu-rs, Rust, built by default) |
| DjVu through DjVuLibre | `skwir-djvulibre-worker` | DjVuLibre (`libdjvulibre`) |
| Audio and video | `skwir-media-worker` | GStreamer playback and codec plugins; the spectrum element from the good plugins; ASF from `gst-plugins-ugly`'s [ASF demuxer](https://gstreamer.freedesktop.org/documentation/asf/asfdemux.html), with `gst-libav` for Windows Media codecs; optional FFmpeg (`ffprobe`) for cover art |
| Archives, comics | `skwir-archive-worker` | libarchive (neither it nor 7-Zip loads into Skwir's process) |
| Office documents as pages | | LibreOffice, bubblewrap |
| RTF (or every text document with Pandoc chosen) | | Pandoc, bubblewrap; Word and OpenDocument read as text need nothing |
| SQLite databases | | `libsqlite3`, which Skwir links |
| Mermaid diagrams | `skwir-mermaid-worker` | Nothing (Rust, built by default; separate because its layout engine is about 16 MB) |
| 3D meshes without a stored picture | `skwir-model-worker` | Nothing (Rust, built by default); without it, an installed thumbnailer (f3d) draws them, if any |
| HEIC, AVIF, JPEG XL, raw photos | | A thumbnailer for them (libheif's `heif-thumbnailer`, for example), else ImageMagick (`magick`, or ImageMagick 6's `convert`) and bubblewrap; with neither they show no picture |

Thumbnailers run in bubblewrap when it is installed and can isolate;
ImageMagick runs only there.

## Checks (for contributors)

- `just previews-check` generates temporary audio, ASF video and
  audio-only files, and archives, and runs the real helpers with a
  clocked fake audio sink. It checks tags, spectrum, seek and pause, ASF
  stream detection and playback without a file-name hint, archive member
  reads, path handling, caller-provided output limits and symlink
  rejection. It does not establish audible output quality on a real
  device.
- The archive helper takes every bound as an argument (`--members`,
  `--names`, `--bytes`) from `skwir_platform::inspection::limits`.
- `just media-check` plays a synthetic two-track video with subtitles
  through GStreamer.
- `just djvu-check` makes DjVu documents with DjVuLibre's tools (a
  colour page, a turned one, text, links, an outline, page titles, an
  indirect and a broken document) and runs the same checks on both DjVu
  helpers, measuring where each draws the ink its text layer points at.
- Parser and plugin lifecycle regressions run in the normal suite
  (`just test`).

For checks by hand, generate synthetic files outside the repository:

```sh
python3 tools/previews/check.py --root /tmp/skwir-preview-demo --generate-only
just run /tmp/skwir-preview-demo
```

The generator can copy an installed system font for the font specimen.
Generated media and screenshots are not repository fixtures; delete the
folder afterwards.
