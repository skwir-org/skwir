# ADR-042: bibli is under the Apache License 2.0

Status: accepted, 2026-10-06, as the user chose. Replaces the dual
licence, MIT or Apache-2.0, the workspace had since its first commit;
amends ADR-014, ADR-037 and ADR-040 where they name it.

## Context

bibli was under MIT or Apache-2.0 at the user's option, the Rust
ecosystem's habit, which suits a library others build into their
programs. bibli is an application, and its binary already holds code
under Apache-2.0 alone: the vendored gpui and xdg-mime (ADR-037).
Whoever distributes bibli already follows Apache-2.0's terms (its text
given, the notices kept); MIT alone would not make bibli any simpler to
redistribute.

Under a dual licence the recipient chooses. Every contributor's patent
licence stands, but Apache-2.0's defence does not: whoever sues over a
patent in the work loses that licence, unless they take the work under
MIT instead. Apache-2.0 also says what a contribution is under when its
author says nothing else (section 5), which matters once others
contribute (Phase 6 opens bibli to plugins of others), and grants no use
of the project's name (section 6).

What Apache-2.0 alone costs: it cannot be combined with code under the
GPL version 2 only. Every commit so far is one author's, so the choice
needs nobody else's agreement now; after outside contributions it would.

## Decision

1. **bibli's own code is under the Apache License 2.0 alone**: its text
   in `LICENSE`, `license = "Apache-2.0"` for every crate of the
   workspace; `LICENSE-MIT` is removed. A contribution is under it
   unless its author says otherwise (section 5); there is no contributor
   agreement.
2. **Every dependency built into bibli allows it**, checked over the
   lockfile on 2026-10-06: permissive licences, MPL-2.0, one crate under
   "Apache-2.0 OR GPL-2.0-only" taken as Apache-2.0. The crates under the
   AGPL are MuPDF's, built into its helper alone.
3. **A helper that links a copyleft library is distributed under it**,
   as before, in the version Apache-2.0 can join:
   - the Poppler reader: Poppler's core is under the GPL version 2 or 3
     (Xpdf's terms) and its additions version 2 or later, so with code
     under Apache-2.0 the program is under the GPL version 3
     (`GPL-3.0-only`; it was marked `GPL-2.0-or-later`, more than Xpdf's
     terms allow);
   - the MuPDF reader: the AGPL version 3 or later, unchanged (ADR-014).
   bibli itself links neither. GStreamer and glib (LGPL-2.1-or-later),
   linked dynamically by the player, leave it under Apache-2.0.
4. **The packages say so**: the Debian and RPM fields and copyright
   files, the PKGBUILD (Arch's `bibli` holds the Poppler reader, so it
   lists `GPL-3.0-only` beside Apache-2.0 and PDFium's BSD-3-Clause),
   the AppStream metadata, the Nix package (which holds both readers);
   every package installs `LICENSE`.

## Consequences

- bibli's crates cannot be built into a program under the GPL version 2
  only; the GPL version 3, the LGPL and the permissive licences can
  take them.
- A redistributor who changes a file says so in it (section 4), as
  gpui's code already asked.
- shared-mime-info's data (GPL-2.0-or-later) stays beside bibli, never
  built into it (ADR-037).
- The licence can no longer change without every contributor's
  agreement once others contribute.
