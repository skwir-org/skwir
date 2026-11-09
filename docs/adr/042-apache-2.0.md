# ADR-042: Skwir is under the Apache License 2.0

Status: accepted, 2026-10-06, as the user chose; amended 2026-11-02, as
the user chose (a crate under the LGPL may be built in). Replaces the dual
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

## Amendment (2026-11-02): crates under the LGPL

- **Context:** ogeom, the model helper's STEP reader (ADR-069), moved
  from MIT-or-Apache-2.0 to LGPL-2.1-only with 0.12. Decision 2 admitted
  no copyleft crate built in, so keeping it would have meant staying on
  0.11 for good. The user: the LGPL is fine.
- **Decision:** a crate under the LGPL may be built into a program of
  Skwir's, statically as Rust links. The program stays under its own
  licence, which allows changing it and reverse engineering it to debug
  such changes (LGPL-2.1 section 6); what the LGPL asks of an executable
  holding the library, that its user can build it again with the library
  changed, is met by the complete source of every release published at
  the place its builds are (section 6 d; LGPL-3.0 section 4 d, by
  GPL-3.0 section 6 d). `deny.toml` and `tools/package/about.toml` allow
  the LGPL versions met so far (LGPL-2.1-only); another is added on the
  same terms when a crate brings it.
- **The packages say so:** a package holding such a program names the
  LGPL beside its other licences (the RPM and PKGBUILD fields, the Nix
  package's meta, the Debian copyright's comment), and its notices carry
  the LGPL's text with the crate's.
- **Consequences:** a build handed on without the source beside it (a
  copy of one program alone) must point to that source. A crate under
  the GPL is still not built into an Apache-2.0 program: only a helper
  under the GPL takes it, as decision 3 says.

## Amendment (2026-11-08): the Nix package's licences follow its flags

The Nix package (ADR-124) builds only the readers its flags keep, so its
`meta.license` says what each build holds: Apache-2.0 and the LGPL (the
model helper's STEP reader) always, the GPL with Poppler's or
DjVuLibre's reader, the AGPL with MuPDF's. `default` holds DjVuLibre's,
not Poppler's or MuPDF's. It installs `LICENSE` and the notices of what
its programs link, as decision 4 asks.
