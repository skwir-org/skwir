# ADR-024: Key schemes and the user's keys

Status: accepted, 2026-09-28. Built. Amended 2026-10-05 (scheme keys that
mean a standard gesture) and 2026-10-13 (the Keyboard page edits in
place).

## Context

Users of yazi, ranger or vim drive a file manager with letters (`j`/`k`
move, `space` marks, `v` selects a range, `y`/`x`/`p` copy, cut, paste). A
plugin could bind them, but the result was wrong:

- **No marks.** yazi keeps marks apart from the hovered file. In Skwir an
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

- yazi keys for what Skwir lacked are left out: line modes, links, tasks,
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

### 2026-10-13: the Keyboard page edits in place

The page listed every binding read-only, one row per key, under a form
asking for a command's name, JSON arguments and a `when` expression.
Nothing on it named the commands, three causes gave several rows one
title, its list had no scrollbar, and a record for a plugin turned off
blocked every later save.

- **One row per action, keys as chips.** An action is a command with its
  arguments. Each is listed once in each place its keys work, all its
  keys as chips; a chip says the conditions it adds ("in the trash"),
  or its row says them once when every key shares them. Anywhere, the
  file list, the panels and previews; the keys inside dialogs and menus
  last, folded. One list draws the rows in view, with its scrollbar and
  the caption of the place it is in kept at its top. *Show* lists the
  actions with keys, all actions, or those the user changed; *Find by
  keys* lists what a key does.
- **Titles come from the declaration.** A command declares its forms:
  arguments that make it one thing to do, each with a title
  (`Declared::forms`, `CommandSpec::title_for`). Every title given to an
  action (the Keyboard page, F1, the key strip, menus) is its form's,
  else the command's with the arguments said after it. The shell's own
  forms are the palette's entries and the keys of its places
  (`commands::KEY_FORMS`, the cursor's moves made from their words);
  only a title naming what an argument names elsewhere (a folder to go
  to, a view, a choice's own title) is worked out
  (`keymap::title`). `crates/skwir/tests/keymap_titles.rs` holds that
  every built-in key with arguments has a form and that no place lists
  two actions under one title.
- **Editing.** A chip, or a row's +, opens a bar under the list that
  takes the next key pressed before any binding sees it
  (`App::intercept_keystrokes`: Ctrl+Q is recorded, not run); *Then
  another key* makes a sequence. Keys the desktop keeps never arrive, so
  they can be typed instead. Before saving, the bar says what the keys
  do now where they would work (`settings_keys::verdict`): what they
  replace there; what they leave running beside them (a place that
  binds them itself, since GPUI takes the deepest place first, said once
  for many places; the rest of the window when they are bound in a
  place; the same place under other conditions); a shortcut of the
  user's on them waiting for its plugin; a letter that ends typing a
  name in the file list. Keys of an unrelated place are not mentioned.
  The list walks with the keyboard (`ui.settings_key` in
  `setting_keys`: arrows, Enter to change or add, Delete to remove).
- **What a change writes** (`settings_keys::apply`). New keys for an
  action are a record; new keys in place of a default are that record
  and one freeing the default's keys where they worked; removing a
  default frees its keys; removing or changing one of the user's edits
  that record. A row's way back removes every record that touches it. A
  default hidden by a record of the user's stays listed, struck through;
  Restore removes that record, saying whose it is.
- **Actions without keys** are listed on request and always in a
  search, under "Without keys": the core's and the plugins' commands
  that need no argument and their forms, the shell's as the palette
  offers them (the rest are keys of a place, listed there). What only
  answers an agent's question, and a command called as a keyed action
  already is (the core's twin of a shell command), are left out. *New
  shortcut* binds a command with arguments of its choosing: picked by
  name, its arguments asked as its schema says them (text, a number, a
  switch, a choice), the place and the file list's conditions chosen,
  then the keys. *Edit as records* shows the records as stored.
- **Keys that must stay.** A command may be declared to keep a key
  (`Declared::keeps_a_key`): the palette, closing Settings and moving
  through it, confirming or dismissing a dialog or a plan, closing a
  menu. Its keys change, but never all go: removing its last key in a
  place is refused, and so is a binding that would hide it.
- **A plugin turned off.** A record whose command nothing registers is
  dormant (`keymap::dormant`): kept in the file, bound to nothing (the
  keys under it work meanwhile), not listed; the page says how many
  wait. It comes back with its plugin (the keymap is built again when
  plugins change). The check refuses an unknown command only in a
  record a save adds or changes (a typo), never in one stored unchanged:
  a setting's validator sees the value stored
  (`SettingSpec::validate_with`, `parse_over`). This replaces "reported
  when run": the code had refused every unknown command at each save, so
  a plugin turned off blocked all later changes to the keys.
- A text field's own keys (Backspace, the arrows in a field) stay the
  field's: not listed, not editable.
