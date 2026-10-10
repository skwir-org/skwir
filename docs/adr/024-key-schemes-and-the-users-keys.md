# ADR-024: Key schemes and the user's keys

Status: accepted, 2026-09-28.

## Context

People who come from yazi, ranger or vim drive a file manager with
letters: `j` and `k` move, `space` marks a file, `v` selects a range, `y`,
`x` and `p` copy, cut and paste. Nothing in skwir stopped a plugin from
binding those keys, but four things made the result wrong:

- **The selection had no notion of marks.** In yazi a hovered file and the
  selected ones are apart: moving leaves the marks alone, and a command
  acts on the marks, or on the hovered file when nothing is marked. In
  skwir an arrow replaces the selection with the item reached and Ctrl+arrow
  moves only the cursor, so neither is yazi's `j`: the first loses the
  marks, the second leaves nothing to act on once the marks are cleared.
- **Nothing held a range while the cursor moved** (visual mode).
- **Letters typed in the file list jump to a name** (type-ahead). A scheme
  that binds most letters would leave the others jumping.
- **Precedence depended on the order plugins were enabled in.** The
  shell's own bindings are listed after the plugins' so they win at the
  same place; a scheme must win over both, whatever was toggled when.

ADR-010 left the Keyboard page read-only: "editing bindings needs a
persisted keymap layer and conflict detection, which is a separate
decision". Remapping is that decision, and it meets the same precedence
question.

## Decision

**The selection knows whether it was picked** (`Selection::picked`). An
arrow, a click and anything that selects one item make a selection that
follows the cursor; toggling, adding, ranges, select all and invert pick.
Two select modes use it, and the standard keys use neither, so they behave
as before:

- `follow`: a move. A selection that follows the cursor moves with it, a
  picked one stays where it is.
- `pick`: yazi's mark. A selection that followed the cursor is not a pick,
  so the first pick starts from nothing; unpicking the last item returns the
  selection to the cursor.

**A sweep is a range held while the cursor moves** (`Selection::sweep`):
`select_sweep` starts one from the cursor, over what was picked before
(selecting, or with `unset` deselecting, what lies between its start and
the cursor); a `follow` move redraws it; `select_sweep_end` keeps what it
covers. Any other change to the selection ends it. The pane's key context
says `sweep` while one lasts, and the status bar shows it: a mode is state.
Both live in the core because agents and scripts select too.

**Keys come in layers**, lowest first: the plugins' defaults
(`Registry::keymap`), the shell's own, key schemes (`Registry::schemes`),
the user's (`keyboard.bindings`). A later layer wins where both apply,
whatever order plugins were enabled in. A binding a later one hides (same
keys, in a place at least as wide) is left out of every list the interface
shows, so a menu or a tooltip never names a key that does something else.

**A scheme is a plugin**, off by default (`Plugin::enabled_by_default`):
the `vim` plugin binds yazi's keys in the file list. It contributes only
bindings; everything it binds is a command anyone can run (`ui.cursor`
for the moves, `ui.cursor_pick`, `ui.find`, `new_file`). Type-ahead yields to the keymap: while any binding takes an unmodified
character in the file list (a scheme's, or the user's), letters are
commands there, and `/` finds a name instead (`ui.find`).

**The user's keys are a setting** (`keyboard.bindings`, declared by the
core so the CLI and agents see and edit it): a list of records, each keys,
a command, its arguments as JSON and where it applies (`when`). A record
without a command unbinds the keys in that place: the key then does what it
would do with nothing bound there. The Keyboard page edits the list with
the records editor, above the shortcuts it lists, and each listed shortcut
says when it is the user's. The keys are checked when saved; a command
that no plugin registers stays saved (its plugin may be off) and is
reported when run.

**Pending keys are shown.** A binding of several strokes (`g h`, `, m`)
waits for the next key without a timeout, as in yazi (vendored patch
0010: `App::set_key_sequence_timeout`), and a strip at the foot of the
window lists what can follow.

## Consequences

- Yazi's keys that name something skwir does not have are left out: line
  modes, links, tasks, the trash as a place, sorting by birth time,
  extension or at random. Its arrows Left and Right keep each view's
  meaning (a grid moves sideways), and Tab keeps moving the keyboard to
  the toolbar or the other pane.
- Conflict detection is precedence, not refusal: a user's binding may
  replace any other, the list shows the one that wins, and removing the
  record brings the default back.
- A record whose keys GPUI cannot parse is refused at save, with the reason
  on the row; the parser in the core follows GPUI's
  (`skwir_core::keymap::valid_keys`),
  and a key it accepts that GPUI does not is skipped with a warning.
- Standard keys are unchanged: arrows still select the item reached, and
  type-ahead works while no letter is bound in the file list.
