# ADR-059: Archives are made of what skwir hands the helper

Status: accepted, 2026-10-13. Revises ADR-048 (attributes put back,
passwords asked, a name taken asked about). Built.

## Context

Skwir extracted archives (ADR-048) and made none. Other file managers
make them:

- Dolphin (Ark): Compress ▸ Here as TAR.GZ, Here as ZIP, and Compress
  to…, a dialog of name, format, level, password and volumes.
- GNOME Files: Compress…, a name and a format (.zip, encrypted .zip,
  .tar.xz, .7z).
- macOS: Compress, a `.zip` with no question.
- Windows 11: Compress to ▸ ZIP, 7z, TAR, and a dialog for the rest.

Extraction failed on an encrypted archive ("open it in an archive
application") and on a name taken ("is already there"). A split had keys
to copy and move across (Shift+F5, Shift+F6), none to extract across.

## Decision

1. **The helper makes them** (`--create`), with libarchive's writers: no
   new library, the same on every system.
   - Formats: ZIP, 7z (LZMA2), tar, and tar compressed by gzip, bzip2,
     xz or Zstandard, at libarchive's default levels.
   - Not RAR (no free writer), split volumes, levels or methods.
   - Not changing an archive in place: libarchive writes it whole again,
     and Undo would have to keep the old one.
2. **Skwir reads the files; the helper opens none.** The step walks what
   goes in (`fs::stat::own`, a folder held open) and hands the helper
   each entry's line and a file's bytes on its standard input. The helper
   runs isolated where the system can (`sandbox::fed`), writing only the
   archive in its staging folder, and a 7z's temporary file beside it
   (not the sandbox's `/tmp`, which is memory).
   - What goes in is what a copy carries: folders whole; a link as a
     link; a file's companions with it (ADR-028).
   - A file's other names are links to the first in a tar, and stored
     whole in a ZIP or a 7z, which keep no hard links.
   - A pipe, a socket or a device is left out, never opened.
   - The attributes a copy keeps (`fs::xattr::kept`: tags among them) go
     into a tar's pax headers. A ZIP and a 7z keep none: their tags are
     lost, unless kept in companion files.
3. **A step of a plan, as extraction is** (`archive.compress`, kind
   `compress`).
   - It is made in a staging folder in the destination and placed whole,
     never over what is there.
   - Cancel and Pause answer while the helper works: skwir stops handing
     it bytes.
   - Undo puts it in the trash. Applied again, an archive listing what the
     walk finds, at those sizes, is done already.
4. **Named after what it holds,** so that Extract Here gives back what was
   compressed.
   - One folder: `photos.zip`.
   - One file: `report.pdf.zip`, as macOS names it; extracted, it is the
     file itself, not a folder around it (ADR-048's single member).
   - Several: the folder holding them (`Holiday.zip`), else `Archive.zip`.
   - The name's ending says the format (`archive::Format::of_name`).
   - Two entries of one name are both kept, the second numbered.
5. **The menu:** three items beside Extract Here, for files and folders,
   not places or the trash.
   - Compress to ZIP asks nothing: ZIP opens everywhere unaided, and a
     name taken is kept beside (`photos (2).zip`).
   - Compress… asks the name in `ui.ask`, the stem selected: the ending
     typed chooses the format, said under the field (`note`). One field
     where GNOME has two; scripts give the same name.
   - Compress with a Password… (under More) makes an encrypted ZIP.
6. **What a step makes takes part in names taken (ADR-054).** A step that
   makes something under a name says so (`Operation::makes`, `made_as`);
   the dry run settles it as a copy's, with a file's choices.
   - Keep both makes it beside: `photos (2)`, `photos (2).zip`
     (`clash::kept_name` with no word).
   - Replace puts what is there in the trash.
   - It is never merged: an extraction merged into a folder would need
     each file it placed undone apart, and the dialog's Merge is a
     copy's. The dialog's button says the step's verb (Extract, Compress).
   - An extraction kept beside unwraps the archive's single member by the
     archive's own name, placing it under the new one.
7. **Passwords are asked when needed, never kept.**
   - A step asks the user through the executor (`StepContext::ask_secret`,
     `App::ask_secret`), and the window's prompt asks it masked
     (`PromptKind::Secret`).
   - Extracting, libarchive asks for one when it meets an encrypted
     member; a wrong one is asked again ("Wrong password").
   - Making an encrypted ZIP (AES-256), it is asked twice, to be sure.
   - The answer reaches the helper through its standard input, never its
     arguments or the plan: Redo asks again.
   - Cancel cancels the plan. With no window (a script, an agent), no one
     is asked, and the step says where it would be.
   - libarchive decrypts ZIP (traditional and AES), not 7z or RAR, and
     encrypts only ZIP.
8. **Extraction puts back the attributes a copy keeps** (the user's on
   Linux, all but the system's own on a Mac), and no others. A tar made by
   skwir brings its tags back. bsdtar puts back none unless run as root.
   An archive's attributes are its author's, as a file copied from
   someone carries theirs.
9. **Send to the other pane… (Shift+F7):** the menu a drop asks with,
   for the selection and the other pane's folder. It offers what plugins
   add for what is sent (Extract Here for an archive), so extracting
   across a split needs no command of its own.

## Consequences

- `skwir-archive-worker` writes archives; its `--probe` is unchanged.
- A 7z takes its size twice on the disk while it is made.
- An agent cannot give a password. An archive needing one is extracted
  from the window.
- Plans whose steps make something are settled even with no copy in them
  (`clash::takes_names`).
- On Windows the helper runs unsandboxed, as it does to extract (ADR-039);
  macOS and Windows compile it, untried (ADR-035).
