# ADR-024: Key schemes and the user's keys

Status: accepted, 2026-09-28. Built. Amended 2026-10-05 (scheme keys that
mean a standard gesture).

## Context

Users of yazi, ranger or vim drive a file manager with letters (`j`/`k`
move, `space` marks, `v` selects a range, `y`/`x`/`p` copy, cut, paste). A
plugin could bind them, but the result was wrong:

- **No marks.** yazi keeps marks apart from the hovered file. In skwir an
  arrow replaces the selection, and Ctrl+arrow moves a cursor that leaves
  nothing to act on once marks clear.
- **No range held while the cursor moves** (visual mode).
- **Type-ahead:** unbound letters would still jump to names.
- **Precedence followed plugin enable order;** a scheme must beat both the
  plugins and the shell.

ADR-010 left the Keyboard page read-only pending a persisted keymap layer
and conflict detection. This is that decision.

## Decision

- **The selection knows whether it was picked** (`Selection::picked`).
  Arrows, clicks and single-item selections follow the cursor; toggle, add,
  ranges, select all and invert pick. Two modes use this (standard keys use
  neither):
  - `follow`: a following selection moves with the cursor; a picked one
    stays.
  - `pick`: yazi's mark. The first pick starts from nothing; unpicking the
    last item returns the selection to the cursor.
- **A sweep holds a range while the cursor moves.** `select_sweep` starts
  one at the cursor over what was picked (`unset` deselects instead);
  `follow` moves redraw it; `select_sweep_end` keeps it; any other
  selection change ends it. The key context says `sweep` and the footer
  shows it: a mode is state. Picks and sweeps are core data, since agents
  and scripts select too.
- **Keys come in layers**, lowest first: plugins' defaults
  (`Registry::keymap`), the shell's (`keymap::BUILTIN`), key schemes
  (`Registry::schemes`), the user's (`keyboard.bindings`). A later layer
  wins, whatever the enable order. A binding a later one hides (same keys,
  a place at least as wide) is left out of every list the UI shows, so no
  menu or tooltip names a key that does something else.
- **A scheme is a plugin, off by default** (`Plugin::enabled_by_default`).
  `vim` binds yazi's keys in the file list, only to commands anyone can run
  (`ui.cursor`, `ui.cursor_pick`, `ui.find`…). While any binding takes an
  unmodified character in the file list, type-ahead is off there and `/`
  finds a name.
- **The user's keys are a core setting** (`keyboard.bindings`, so the CLI
  and agents can edit it): records of keys, command, JSON arguments and
  `when`. A record without a command unbinds those keys there. The
  Keyboard page edits the records and marks the user's shortcuts. Keys are
  checked on save; a command no plugin registers stays saved (its plugin
  may be off) and is reported when run.
- **Pending keys are shown.** A multi-stroke binding (`g h`) waits without a
  timeout, as in yazi (vendored patch 0010), and a strip at the window's
  foot lists what can follow.

## Consequences

- yazi keys for what skwir lacked are left out: line modes, links, tasks,
  the trash as a place (one since ADR-034), sorting by birth time,
  extension or at random. Left
  and Right keep each view's meaning; Tab still moves the keyboard to the
  toolbar or the other pane.
- Conflicts resolve by precedence, not refusal: a user binding may replace
  any other, the list shows the winner, removing the record restores the
  default.
- Keys GPUI cannot parse are refused on save, with the reason on the row
  (`skwir_core::keymap::valid_keys`); keys it parses that GPUI does not are
  skipped with a warning.
- Standard keys are unchanged.

## Amendments

### 2026-10-05: a scheme's key can mean a standard gesture

A scheme key standing for a standard gesture binds `keys.like` with the
standard keys (vim's `d` is Delete, `r` is F2) and takes that key's
bindings and conditions, so it follows the place (in a tag `d` takes the
tag off, in the trash it deletes for good) and copies no condition that
could drift.
