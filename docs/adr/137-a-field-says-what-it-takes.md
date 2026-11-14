# ADR-137: A field says what it takes

Status: accepted, 2026-11-14, on the user's answer (asked for help beside
the bulk rename's fields, its template's placeholders and common regular
expressions, as a [?] after the field opening a popup, and where else
one is needed; they chose the four steps recommended below: the
reference itself, the bulk rename's, the search's and Access's). Built
on Linux; compiled for Windows, and for macOS but the window (which
`check-macos` leaves out).

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
2. **A dialog's field has a [?] at its end, and F1 in it** (`reference`
   in its key context, `ui.field_reference`) opens its reference, hanging
   from the [?] over the dialog (`reference.rs`): a popup as any, drawn
   among the window's overlays so the dialog's keys stay its own; its
   rows built of the menu's parts (`Button::said`: what is written in one
   column, cut in its middle, what it means, what it gives;
   `MenuPart::Note`). A row chosen writes its part where the caret is
   (`TextInput::put`) or fills the fields its recipe names, an edit
   Ctrl+Z takes back, and the field has the keyboard again. Escape closes
   the reference alone; the arrows walk its rows, not the dialog's
   fields. F1 there shows the keys of the place.
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
   the field would end what is typed. F1, or the [?] at the end of the
   card's first line, asks the sources as `?` alone does (`Typed::Help`),
   the text kept; the query plugin answers its whole reference
   (`query.reference`), under its groups' headings (`Suggestion::heading`),
   each row writing its part where the caret is (a term apart from the
   words beside it, a sign of a pattern as it is, in place of a lone
   `?`). A key, or Escape, goes back to what the text goes on with; F1
   again shows the keys.
5. **A question says what is typed comes to** (`ui.ask`'s `describe`): a
   command run on a worker with the text as its field, its `says` under
   the field, its error there in the error's color, holding the answer
   back. With `reference`, a question's field has its [?] and F1. Access
   passes both: core says a mode's words (`set_access.describe`,
   `op::access::said`, from the parser and `apply` that change the file)
   and how one is written (`op::access::reference`).

## Consequences

- An agent reads what the window shows: `rename_bulk.reference`,
  `query.reference`, `set_access.describe`, and `ui.ask` takes what the
  window draws.
- A plugin's question gets a reference and live words by passing them;
  no dialog draws one of its own.
- What the reference gives is worked out at each preview (three commands
  beside it, each reading the first selected item alone).
- Left: Settings › Exclusions' patterns and Run Command… have no
  reference yet; Access's words on Windows (the read-only attribute)
  compiled, not tried there.
