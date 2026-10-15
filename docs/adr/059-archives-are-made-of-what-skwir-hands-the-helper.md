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
   (not the sandbox's `/tmp`, which is memory: `TMPDIR` names it, and
   `TMP` and `TEMP` on Windows, where libarchive asks `GetTempPathW`).
   - Each file handed is the one walked: its identity, size and time as
     the walk found them when it is opened, its size and no more read,
     its time unchanged after. Another file at its name (a link put
     there, a file saved anew), or one written to meanwhile, stops the
     step: "changed as it was read". A file is never cut short in an
     archive.
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
   - Undo puts it in the trash. Applied again, it is done already when
     the archive is told done as a copy is (`same_tree`): it lists what the
     walk finds and nothing else, each kind alike, a file at its size and
     its time, a link saying the same. Times are compared to the second, as
     a tar and a ZIP's extended time keep them; a ZIP without its extended
     time keeps a local time with no zone, to two seconds, read in this
     machine's zone: one made elsewhere is not done, and the name taken is
     asked about. A file changed in place, its size kept, makes it again.
   - Room is checked as for a copy (`Operation::writes`): what goes in,
     stored whole (a tar's other names once), and twice that for a 7z.
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
     where GNOME has two; scripts give the same name. It is the user's
     alone (`user_only`): the prompt runs what is answered as the user's
     own action, and does not show the folder, which an agent would
     choose. An agent runs `archive.compress`, a plan it proposes.
   - Compress with a Password… (under More) makes an encrypted ZIP.
6. **What a step makes takes part in names taken (ADR-054).** A step that
   makes something under a name says so (`Operation::makes`, `made_as`);
   the dry run settles it as a copy's, with a file's choices.
   - Keep both makes it beside: `photos (2)`, `photos (2).zip`
     (`clash::kept_name` with no word).
   - Replace puts what is there in the trash, never when it is, or
     holds, what the step reads (`Made::inputs`: an earlier archive among
     what is compressed, a folder holding the archive extracted): the
     step's check says so instead, and nothing is done.
   - Replace if newer compares what is there with the latest change
     among what the step reads, a file deep in a folder as well as the
     folder.
   - It is never merged: an extraction merged into a folder would need
     each file it placed undone apart, and the dialog's Merge is a
     copy's. The dialog's button says the step's verb (Extract, Compress).
   - An extraction kept beside places the archive's single member under
     the new name. A file's name keeps its extension last
     (`report (2).pdf` of `report.pdf.zip`): the step reads the archive's
     listing to say what it makes (`makes`, given the plan's context;
     ADR-048's amendment of 2026-10-15).
   - Extracted, done is told as for a copy too: what was made holds
     what the archive lists and nothing else, each file at its size and
     at its time to two seconds (a FAT drive's), each link saying the same,
     each hard link to its first name. A member of no size or time said
     (a file only compressed) cannot be told done: the name is asked
     about.
   - Room is checked against the archive's listing: its members' sizes,
     a lower bound when the listing was cut short. A compressed tar not
     listed before (by the preview) is read once to be counted.
7. **Passwords are asked when needed, never kept.**
   - A step asks the user through the executor (`StepContext::ask_secret`,
     `App::ask_secret`), and the window's prompt asks it masked
     (`PromptKind::Secret`).
   - Extracting, libarchive asks for one when it meets an encrypted
     member; a wrong one is asked again ("Wrong password"). Each member's
     attempts count from the first: one of another password is asked as a
     first. An empty answer is asked again, as making one does.
   - A member whose bytes are not those stored (a ZIP's CRC or AES
     authentication that does not match, a size other than declared)
     stops the extraction, whatever libarchive's version calls it (3.6, in
     the AppImage, a warning). Encrypted, it says "wrong password or
     damaged archive": a wrong password passes ZipCrypto's one-byte check
     once in 256, and decrypts to what no CRC matches. A preview of such
     a member fails alike.
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
   - What a drive refuses to keep (attributes, times on a drive without
     them) is left out, as a copy leaves it: any other failure to put a
     member's metadata back stops the extraction.
   - On a Mac, not what says which application opens a file
     (`com.apple.LaunchServices.*`), the archive's author's choice. What
     is extracted from an archive held in quarantine (downloaded) takes
     the archive's `com.apple.quarantine`, every file, folder and link,
     as the Mac's Archive Utility does: Gatekeeper asks before what came
     in it first runs. This is compiled on macOS, never run there yet.
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
  macOS and Windows compile it, untried (ADR-035): the Mac's quarantine
  and the Windows temporary folder above are compiled, never run.
