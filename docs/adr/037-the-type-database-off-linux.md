# ADR-037: Each system's own types first, shared-mime-info behind them

Status: accepted, 2026-10-06 (Phase 5, step E; ADR-035). Proposed
2026-10-05 with three options; the user chose: the system's own type
registry when it can answer, shared-mime-info as the fallback.
Amended the same day by ADR-042: bibli is under Apache-2.0 alone, which
changes nothing here (the database stays data beside bibli).

## Context

bibli types files with the desktop's database, shared-mime-info: by
name (globs, weights, case), by content (magic, where a name is
ambiguous), with its aliases, its hierarchy (`subclass-of`, which
`kind:` and every provider's "takes files by type" rely on) and its
generic icons (which group types into kinds). Linux has it installed;
it is read through `xdg-mime`. Without it, `kind:`, previews and
thumbnails chosen by type, and the stored pictures are off.

macOS and Windows do not ship it, and their own registries answer
less, and otherwise. macOS's Uniform Type Identifiers declare types
with MIME names, extensions, localized names and a conformance graph,
but read no bytes; Windows' registry maps an extension to a
`Content Type` and a coarse `PerceivedType`, names it in Explorer's
words, and has no hierarchy and no content sniffing worth the name
(`FindMimeFromData` knows a few dozen web types). shared-mime-info's
data is GPL-2.0-or-later, bibli MIT or Apache-2.0: it can be shipped
beside bibli as data of its own (as every Linux distribution ships it
beside programs that read it), never built into it.

The options were: (1) ship the database and read only it; (2) map
each system's types onto bibli's and ship no GPL data; (3) a small
permissive table.

## Decision

1. **The system's registry answers first where it can; the database
   answers the rest** (`platform::mime`). On Linux and the BSDs the
   database is the system's registry: nothing changes there.
   - *What a file is, by its name*: the system's type for its
     extension (macOS: the declared, not dynamic, `UTType` and its
     preferred MIME name; Windows: `HKEY_CLASSES_ROOT\.ext`'s
     `Content Type`). The database may only say it more precisely, by
     a type that descends from the system's (a `.tar.gz` the system
     calls gzip is a compressed tar; a `.js` an editor registered as
     plain text is JavaScript; the Ogg types the bytes choose among
     stay). Where it contradicts the system, the system wins: a
     `.ts` file is what Windows says it is, as in Explorer. A name the
     system does not know (no extension, a whole-name pattern such as
     `Makefile`) is the database's.
   - *What the bytes say* is the database's magic alone, then the
     text-or-binary rule, on every system.
   - *What people call a type* is the system's, in the user's language
     (`localizedDescription`, Explorer's `FRIENDLYDOCNAME`); the
     database's untranslated comment where the system has none.
   - *Its names*: the database's aliases first, whatever the system,
     because the database's names are the words providers and queries
     speak (`mime::Types`, `kind:`): a system's answer is put in them
     (Windows' `application/x-zip-compressed` is `application/zip`).
     A type the database does not know is named as the system
     prefers.
   - *Its kinds*: on macOS what the type conforms to (`public.image`,
     `public.spreadsheet`… a short table of the system's own groups)
     adds to what the database's generic icons say; Windows'
     `PerceivedType`, which has no spreadsheets, presentations or
     fonts and calls Office files documents, groups only what the
     database gives no kind.
   - *Its extensions*: the system's first, then the database's.
   - *Its hierarchy* (`lineage`) is the database's: macOS's
     conformance graph is mostly types without MIME names
     (`public.data`, `public.content`), Windows has none.
2. **macOS and Windows packages carry the database** beside bibli as a
   folder of its own (`share/mime`): only the compiled files bibli
   reads, 180 KB (43 KB compressed), without the per-type XML (each
   type's name in every language, 7 MB installed), since there the
   system names types; with its licence and a note of where its source
   is, not the source itself, which would weigh more than the files.
   The source is vendored (`vendor/shared-mime-info`: its type
   definitions, unchanged, release 2.5.1, and its licence), and the
   packages' files are compiled from it by shared-mime-info's own
   `update-mime-database` (`tools/package/mime-database.sh`; the dev
   shell has the program, a Mac Homebrew's), so what ships is always
   what the repository holds. Linux packages carry none: the desktop's
   is the system's registry there. bibli finds it beside the program,
   a bundle's `Resources/share`, Homebrew's, or `$XDG_DATA_DIRS`, the
   first found.
3. **The reader runs everywhere**: `xdg-mime` is vendored
   (`vendor/xdg-mime`, Apache-2.0) with one patch lifting its Unix-only
   gate; it reads files by name and holds nothing of Unix's. Its own
   tests' data (a copy of the GPL database) is left out of the tree.
4. The system is asked once per extension or type and its answers
   kept, bounded (`MEMO`), so typing every row stays cheap.

## Consequences

- A file can have another type on macOS or Windows than on Linux, and
  `kind:` can group a type otherwise there: what that system's other
  applications say, which is ADR-000's fourth principle. An extension
  some program registered wrongly is typed wrongly in bibli too, as
  in Explorer.
- Without the database (a build run from source with none installed)
  macOS and Windows still type files by name and name them, but read
  no bytes, know no hierarchy beyond the system's, and `kind:` is the
  system's alone.
- The packages hold GPL data under its own licence, and the
  repository its source: whoever redistributes them redistributes it
  with its licence, as they would on Linux.
- Tested: the composition on Linux (`refine`); the database compiled
  from the vendored source and read on Windows under Wine from beside
  the test programs, as a package puts it; the registry's types, kinds, extensions and Explorer's name
  under Wine, in its scratch prefix (writing the registry, the test
  runs nowhere else). macOS's `UTType` reader is compiled, never run
  (5b), and its conformance table unverified.
