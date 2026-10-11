# ADR-042: Skwir is under the Apache License 2.0

Status: accepted, 2026-10-06, as the user chose. Replaces the dual
licence (MIT or Apache-2.0) held since the first commit; amends ADR-014,
ADR-037 and ADR-040 where they name it.

## Context

- MIT-or-Apache-2.0 is the Rust habit and suits libraries. Skwir is an
  application whose binary already holds Apache-2.0-only code (vendored
  gpui and xdg-mime, ADR-037), so distributors already follow
  Apache-2.0's terms; MIT would not simplify redistribution.
- Under a dual licence the recipient chooses: every contributor's patent
  licence stands, but Apache-2.0's patent defence (suing over a patent
  in the work ends that licence) is lost to whoever takes MIT.
- Apache-2.0 sets what a contribution is under when its author says
  nothing (section 5), which matters once others contribute (Phase 6
  opens Skwir to others' plugins), and grants no use of the name
  (section 6).
- Cost: it cannot combine with GPL-2.0-only code.
- All commits so far are one author's, so no one else must agree now;
  after outside contributions they would.

## Decision

1. **Skwir's own code is Apache-2.0 alone:** `LICENSE`,
   `license = "Apache-2.0"` in every workspace crate, `LICENSE-MIT`
   removed. Contributions are under it unless their author says
   otherwise (section 5). No contributor agreement.
2. **Every dependency built into Skwir allows it** (lockfile checked
   2026-10-06): permissive licences, MPL-2.0, one "Apache-2.0 OR
   GPL-2.0-only" crate taken as Apache-2.0. The AGPL crates are MuPDF's,
   in its helper alone.
3. **A helper linking a copyleft library is distributed under it**, in
   the version Apache-2.0 can join:

   | Helper | Library's terms | Helper's licence |
   |---|---|---|
   | Poppler reader | core GPL-2 or GPL-3 (Xpdf's terms); additions GPL-2.0-or-later | `GPL-3.0-only` (was marked `GPL-2.0-or-later`, more than Xpdf's terms allow) |
   | MuPDF reader | AGPL-3.0-or-later | AGPL-3.0-or-later (ADR-014) |

   Skwir links neither. GStreamer and glib (LGPL-2.1-or-later), linked
   dynamically by the player, leave it Apache-2.0.
4. **The packages say so:** Debian and RPM fields and copyright files;
   the PKGBUILD (Arch's `skwir` holds the Poppler reader: `GPL-3.0-only`
   beside Apache-2.0 and PDFium's BSD-3-Clause); AppStream metadata; the
   Nix package (both readers). Every package installs `LICENSE`.

## Consequences

- Skwir's crates cannot go into a GPL-2.0-only program; GPL-3.0, LGPL
  and permissive ones can take them.
- A redistributor who changes a file says so in it (section 4), as
  gpui's code already asked.
- shared-mime-info's data (GPL-2.0-or-later) stays beside Skwir, never
  built in (ADR-037).
- Once others contribute, changing the licence needs all of them.
