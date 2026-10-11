# ADR-036: A file's identity on Windows

Status: accepted, 2026-10-05. Part of Phase 5's step B (ADR-035).
Written on Linux, run under Wine only. Point 1 revised by ADR-049:
device and inode identify a file (`FileId`), not an entry.

## Context

An entry's identity (`EntryId { dev, ino }`, ADR-000's principle 6) is
what a stat reports: device and inode. Listings key rows by it,
`EntryRef` resolves moved files by it, and the thumbnail memory, fields
cache, tags index and Usage remember files by it. A listing names an
entry by the folder's device and `d_ino` before any stat, and the stat
must agree.

Windows gives the volume serial and file ID through a handle, in two
records:

| Record (stat / listing) | Serial | ID | Given by |
|---|---|---|---|
| `FILE_ID_INFO` / `FILE_ID_EXTD_DIR_INFO` | 64-bit | 128-bit | NTFS (low 64 bits: its file reference), ReFS (all 128) |
| `BY_HANDLE_FILE_INFORMATION` / `FILE_ID_BOTH_DIR_INFO` | 32-bit | 64-bit index | FAT and some network filesystems (only this one) |

Opening every file to ask would cost a 100 000-entry listing 100 000
opens; the listing records carry the ID.

## Decision

1. **`dev` is the volume serial, `ino` the file ID**, from the 128-bit
   record where the filesystem gives it, else the older one. A volume
   answers one way for all its files, so listing and stat agree.
2. **A 128-bit ID is folded to 64 bits**: low half XOR high half
   rotated by 32. NTFS IDs (high half zero) stay exact. ReFS IDs could
   collide in principle; none has been seen to. Rejected: widening
   `EntryId`, which touches every key and serialized plan, for a case
   not seen.
3. **A listing reads IDs from the folder**
   (`GetFileInformationByHandleEx`: `FileIdExtdDirectoryInfo`, else
   `FileIdBothDirectoryInfo`), opening no file. A stat opens the file
   (`FILE_FLAG_OPEN_REPARSE_POINT` for a link itself) and asks it.
4. **A folder is held open with every share mode**, delete included, so
   holding it never blocks another program renaming or removing it, as
   on Unix. Its listing uses its own handle, opened at the folder's
   final path (`GetFinalPathNameByHandleW`, following a move), so two
   readings never share a position. Rejected: `ReOpenFile` (keeps the
   very file open; Wine refuses it for folders).

## Consequences

- FAT IDs are the record's position in its folder and change when a
  file is renamed into another folder; an `EntryRef` there resolves by
  its path hint, as on FUSE filesystems with unstable inodes.
- `std::fs::Metadata` exposes no stable identity on Windows:
  `stat::identity` answers `None` there; callers ask by path
  (`stat::id_of`).
- Wine is not NTFS: the folding and the fallback are checked on Windows
  in 5b.
