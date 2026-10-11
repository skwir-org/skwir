# ADR-037: Each system's own types first, shared-mime-info behind them

Status: accepted, 2026-10-06 (Phase 5, step E; ADR-035). Proposed
2026-10-05 with three options; the user chose the system's registry
where it can answer, shared-mime-info as the fallback. Built: tested on
Linux and under Wine; the macOS reader is compiled, never run. ADR-042
(Skwir under Apache-2.0 alone) changes nothing here.

## Context

Skwir types files with shared-mime-info: by name (globs, weights,
case), by content (magic), with aliases, a hierarchy (`subclass-of`,
which `kind:` and "takes files by type" rely on) and generic icons
(which group types into kinds). Linux has it, read through `xdg-mime`.
Without it, `kind:`, type-chosen previews and thumbnails, and stored
pictures are off.

macOS and Windows do not ship it, and their registries answer less:

- **macOS** UTTypes have MIME names, extensions, localized names and a
  conformance graph, but read no bytes.
- **Windows** maps an extension to a `Content Type` and a coarse
  `PerceivedType`, names it in Explorer's words, and has no hierarchy
  and almost no sniffing (`FindMimeFromData`: a few dozen web types).

The database is GPL-2.0-or-later (Skwir was then MIT or Apache-2.0): it
can ship beside Skwir as data, as Linux distributions ship it, never
built in.

Options: (1) ship the database and read only it; (2) map each system's
types onto Skwir's, no GPL data; (3) a small permissive table.

## Decision

1. **The system's registry answers first where it can; the database
   the rest** (`platform::mime`). On Linux and the BSDs the database is
   the system's registry: nothing changes.

| Question | Answered by |
|---|---|
| Type by name | The system's type for the extension (macOS: declared `UTType`, its preferred MIME name; Windows: `HKEY_CLASSES_ROOT\.ext` `Content Type`). The database may only refine it with a descendant (`.tar.gz` gzip → compressed tar; `.js` as plain text → JavaScript; Ogg types chosen by bytes). On contradiction the system wins (`.ts` is what Windows says). Names the system does not know (no extension, `Makefile`) are the database's. |
| Type by bytes | The database's magic, then text-or-binary, everywhere. |
| Display name | The system, in the user's language (`localizedDescription`, `FRIENDLYDOCNAME`); else the database's untranslated comment. |
| Canonical name | The database's aliases, since providers and queries speak them (`mime::Types`, `kind:`): Windows' `application/x-zip-compressed` is `application/zip`. Unknown to the database: the system's name. |
| Kind | macOS: conformance (`public.image`, `public.spreadsheet`…, a short table) adds to the generic icons. Windows' `PerceivedType` (no spreadsheets, presentations or fonts; Office files are "documents") only fills types the database gives no kind. |
| Extensions | The system's, then the database's. |
| Hierarchy (`lineage`) | The database's: macOS's graph is mostly types without MIME names; Windows has none. |

2. **macOS and Windows packages carry the database** in `share/mime`:
   only the compiled files Skwir reads (180 KB, 43 KB compressed), not
   the per-type XML (7 MB of translated names the system provides
   there); its licence and a note of where its source is (the source
   itself would outweigh the files). The source is vendored
   (`vendor/shared-mime-info`, release 2.5.1, unchanged, with its
   licence) and compiled by its own `update-mime-database`
   (`tools/package/mime-database.sh`; the dev shell's, on a Mac
   Homebrew's), so what ships is what the repository holds. Linux
   packages carry none. Skwir looks beside the program, in a bundle's
   `Resources/share`, Homebrew's, then `$XDG_DATA_DIRS`: the first
   found.
3. **The reader runs everywhere**: `xdg-mime` is vendored
   (`vendor/xdg-mime`, Apache-2.0) with one patch lifting its Unix-only
   gate. Its tests' data (a copy of the GPL database) is left out.
4. The system is asked once per extension or type, answers kept,
   bounded (`MEMO`).

## Consequences

- A file can have another type or kind on macOS or Windows than on
  Linux: what that system's applications say (ADR-000, principle 4). A
  wrongly registered extension is wrong in Skwir too, as in Explorer.
- Without the database (a source build with none installed), macOS and
  Windows still type and name files by name, but read no bytes, know no
  hierarchy beyond the system's, and `kind:` is the system's alone.
- The packages hold GPL data under its licence, and the repository its
  source; redistributors carry it with its licence, as on Linux.
- Tested: the composition on Linux (`refine`); the vendored database
  compiled and read under Wine from beside the test programs, as
  packaged; the registry's types, kinds, extensions and Explorer's name
  under Wine, in its scratch prefix (the test writes the registry, so
  runs nowhere else). macOS's `UTType` reader is compiled, never run
  (5b); its conformance table is unverified.
