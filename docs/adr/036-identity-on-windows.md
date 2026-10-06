# ADR-036: A file's identity on Windows

Status: accepted, 2026-10-05. Part of Phase 5's step B (ADR-035).

## Context

An entry's identity (`EntryId { dev, ino }`, ADR-000's principle 6) is
what a stat reports: the device and the inode, on Linux and macOS alike.
Listings key their rows by it, `EntryRef` resolves a moved file by it,
and the thumbnail memory, the fields cache, the tags index and Usage
remember files by it. A listing names an entry by the folder's device
and the inode the folder lists (`d_ino`) before anything is stat-ed, and
the stat must agree.

Windows says the same through a handle: the volume's serial number and
the file's ID. Two records hold them. `FILE_ID_INFO` (and the listing's
`FILE_ID_EXTD_DIR_INFO`) carries a 64-bit serial and a 128-bit ID; NTFS
fills the ID's low 64 bits (its file reference), ReFS uses all 128. The
older `BY_HANDLE_FILE_INFORMATION` (and the listing's
`FILE_ID_BOTH_DIR_INFO`) carries a 32-bit serial and a 64-bit index,
which is all FAT and some network filesystems give. Opening each file
to ask would cost a listing of 100 000 entries 100 000 opens; the
listing's records carry the ID, so it needs none.

## Decision

1. **`dev` is the volume serial, `ino` the file ID**, both from the
   128-bit record where the filesystem gives it, from the older one
   where it does not. A volume answers one way or the other for all its
   files, so a listing and a stat of it agree.
2. **A 128-bit ID is folded into 64 bits**: the low half XOR the high
   half rotated by 32. NTFS's IDs, whose high half is zero, are kept
   exactly; ReFS's may in principle collide, which would make two files
   one in a listing. No ReFS volume has been seen to; widening `EntryId`
   for it would touch every key and serialized plan, a cost for a case
   not seen.
3. **A listing reads the IDs from the folder** (`GetFileInformationByHandleEx`
   with `FileIdExtdDirectoryInfo`, else `FileIdBothDirectoryInfo`), never
   opening a file; a stat opens the file (with
   `FILE_FLAG_OPEN_REPARSE_POINT` for the link itself) and asks it.
4. **A folder is held open with every share mode**, delete included,
   so holding it never stops another program renaming or removing it,
   as on Unix; its listing reads through a handle of its own, opened at
   the folder's final path (`GetFinalPathNameByHandleW`, so a moved
   folder is followed), so two readings never share a position.
   (`ReOpenFile` would keep the very file open; Wine refuses it for a
   folder.)

## Consequences

- FAT's IDs are where a file's record sits in its folder: they change
  when a file is renamed into another folder. An `EntryRef` there
  resolves by its path hint, as on a Unix filesystem whose inodes are
  not stable (some FUSE ones).
- Code that holds only `std::fs::Metadata` cannot read an identity on
  Windows (`std` keeps the serial and index unstable): `stat::identity`
  answers `None` there, and such callers ask a path (`stat::id_of`) when
  they need one.
- Written on Linux and run under Wine; Wine is not NTFS, so the folding
  and the fallback are checked on Windows in 5b.
