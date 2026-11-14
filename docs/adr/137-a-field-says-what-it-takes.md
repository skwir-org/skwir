# ADR-137: A field says what it takes

Status: accepted, 2026-11-14, on the user's answer (asked for help beside
the bulk rename's fields, its template's placeholders and common regular
expressions, as a [?] after the field opening a popup, and where else
one is needed; they chose the four steps recommended below: the
reference itself, the bulk rename's, the search's and Access's). Built
on Linux; compiled for Windows, and for macOS but the window (which
`check-macos` leaves out). Amended the same day, the user asking for the
`?` inside the field, one look everywhere, and for what was recommended
beside it: a field offers, as it is typed, what goes on with a part
begun (6). Amended again on 2026-11-15, the user finding the reference's
panel darker than F1's and the search's list, its `?` hard to read, the
address's not at the box's end, and asking for Settings › Exclusions and
Run Command…: the reference is the list under the field, the `?` a plain
one, and every view's field holds the same component (2, 7).

## Context

- **The bulk rename's fields said what they take in their placeholder
  alone**, gone as soon as anything was typed, and wrong: the template's
  named `{stem}`, which the plugin refuses, and never `{full}` or
  `{parent}`. Its regex and replacement said nothing of the `regex`
  crate's syntax, where `$1_x` reads as a group named `1_x` and gives
  nothing. The dialog opened on `{name}`, the name without its
  extension: Enter at once took every extension away.
- **An agent read no more**: `rename_bulk`'s schema gave the template no
  description at all.
- **The address field's `?` listed ten search terms**, only with `?`
  alone in the field: nothing of how terms go together (`OR`, `NOT`, `-`,
  quotes), of values (sizes, periods, ranges), of names' wildcards or of
  a regex in `content:/…/`, and nothing once words were typed.
- **Properties' Access field** said its syntax in its placeholder, gone
  once typing, and what a mode came to only once applied; a mistyped one
  was refused by the plan.
- What a field reads is its owner's language: the plugin's (the template,
  the query language), or core's (`chmod`'s mode, `op::access`). The
  shell names no plugin and knows no query syntax.

## Decision

1. **What a field takes is data its owner says**
   (`skwir_core::reference`): a `Reference` of groups, each of items, each
   as it is written, what it means, what it gives for what the field is
   about (the first file's), and what choosing it writes: put where the
   caret is (`inserts`), or written whole in the fields a recipe names
   (`fills`). The signs of a regular expression, which two plugins read
   with one crate, are said once there (`regex_pattern`).
2. **A field that says what it takes has a `?` inside its frame at its
   end** (first a [?] after the field), as a form's list button
   (`inner_chevron`) and the address's own buttons are: what is about
   the field's content lives in it. A plain `?` in the window's font,
   muted (amended: first a symbol in a circle, too small to read), on the
   term's ground while what it opens shows: the sign `?` alone asks the
   address too. It acts on the press and leaves the keyboard in the field,
   no Tab stop. It, or F1 in the field (`reference` in its key context,
   `ui.field_reference`), shows everything the field takes **in the list
   under the field** (amended: first a popup over the dialog, a menu on
   the chrome's ground), as the address's card shows it: on the window's
   ground, each group under its title with its notes, each row as
   written, what it means, what it gives; the keyboard staying in the
   field, its keys those of 6. A row taken writes its part where the
   caret is (`TextInput::put`) or fills the fields its recipe names, an
   edit Ctrl+Z takes back; the `?` again puts the list away, F1 again
   shows the keys of the place.
3. **The bulk rename** asks its plugin with each preview
   (`rename_bulk.reference`, field by field): the template's placeholders
   each with what it gives the first item as the rename stands (after the
   regex), the date formats so, recipes filling the regex and its
   replacement (each with what it makes of the first name), a pattern's
   signs, a replacement's (`${1}` and its trap). A test holds the
   template's reference to what `render` reads. The schema describes
   every field. The preview marks where the regex matched in each old
   name, and what it put in the new one while the template and the case
   left it as it made it (`Marks`). `{.ext}` is the extension with its
   dot, nothing without one. The dialog opens on `{full}`.
4. **The address field says it in its card**: taking the keyboard from
   the field would end what is typed. F1, or the `?` inside the field
   while it is typed in (amended; first at the end of the card's first
   line), asks the sources as `?` alone does (`Typed::Help`),
   the text kept; the query plugin answers its whole reference
   (`query.reference`), under its groups' headings (`Suggestion::heading`),
   each row writing its part where the caret is (a term apart from the
   words beside it, a sign of a pattern as it is, in place of a lone
   `?`). A key, or Escape, goes back to what the text goes on with, as
   the `?` pressed again does; F1 again shows the keys.
5. **A question says what is typed comes to** (`ui.ask`'s `describe`): a
   command run on a worker with the text as its field, its `says` under
   the field, its error there in the error's color, holding the answer
   back, about what the question is about (the `pane`, `entries` and
   `in` its command was given). With `reference`, a question's field
   has its `?` and F1. Access
   passes both: core says a mode's words (`set_access.describe`,
   `op::access::said`, from the parser and `apply` that change the file)
   and how one is written (`op::access::reference`).
6. **A field offers, as it is typed, what goes on with a part begun**
   (amended): a reference names what begins a part (`starts`: `{` in the
   template, `\` and `(?` in a pattern, `$` in a replacement), and the
   items whose part goes on with what is typed since the last of them
   are listed under the field (`Reference::offered`, the window knowing
   no language). The keyboard stays in the field (`offers` in its key
   context, `ui.field_complete`): Up and Down move the highlight from
   the first row, Tab takes the row lit, Enter only one the arrows chose
   (else it answers the dialog), Escape puts the list away until the text
   or the caret moves; a press on a row takes it. The item takes the
   place of the part typed, as an edit. Access says no `starts`: a mode
   is too short to offer anything.
7. **One component for every view's field** (amended,
   `reference::FieldHelp`): what a field takes, its list's state and its
   frame's `?`, keys and list (`with_help`), the frame answering the
   commands its keys run itself, so the view holding the field (`Helps`:
   the shell's dialogs, Settings) is the one asked. **Settings ›
   Exclusions**' name or path, name beside and file held say theirs from
   core (`exclusions::reference`, each example read as the rules read
   it), beside the line saying what the entry means. **Run Command…**
   says its examples and how the paths are given (the shell plugin's
   `reference`), and, as the command is typed, the line that runs with
   the selected paths after it and the folder it runs in
   (`shell.run.describe`); an empty command is held back.

## Consequences

- An agent reads what the window shows: `rename_bulk.reference`,
  `query.reference`, `set_access.describe`, and `ui.ask` takes what the
  window draws.
- A plugin's question gets a reference and live words by passing them;
  no dialog draws one of its own.
- What the reference gives is worked out at each preview (three commands
  beside it, each reading the first selected item alone).
- Left: Access's words on Windows (the read-only attribute) compiled,
  not tried there; Run Command… on Windows (no terminal yet).
