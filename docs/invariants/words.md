# Words invariants: what Skwir says, in every language

Read this before writing anything a person or an agent reads: a label,
a title, a message, an error, a toast, an accessible name, a
reference's item. Binding, like `AGENTS.md`. Reasoning: ADR-139; the
research behind it is `docs/i18n.md`.

Status: being built (PLAN.md, "Words in every language"). "Words two
owners share" and "Nothing reads words back" bind now. The rest binds
each crate from the step that converts it; until then, write new text
so that converting it is mechanical: one literal or one `format!` per
sentence, never a sentence assembled from pieces.

## A message is a whole sentence

- **One message says one thing whole**: a label, a sentence, a title.
  A sentence is never assembled from pieces, whatever joins them
  (`format!`, `push_str`, `concat`, `join`).
- **What varies is an argument the message places.** An argument's
  place in its sentence is the language's to choose, never the code's.
- **Nothing is done to words in code**: no capitalising (a title and
  the same words inside a sentence are two messages), no plural made by
  adding a letter, no list joined with commas or "and", no quotes or
  brackets around a name, no punctuation or space added (French puts a
  space before `:`).
- **The same words in another role are another message**: a menu
  item's "Copy" and a toast's "Copied" are two.
- **A message an agent or a log reads is still a message**: the reader
  chooses the language (below), never the writer.

## Writing one

- **`words!(id = "English", argument = value, …)`.**
  - The English is the message in Fluent's syntax, written where it is
    used. The id is `snake_case`, unique in its crate, and says what
    the message is for, not what it says (`trash_moved`, not
    `moved_n_items_to_the_trash`).
  - Change an id only when the meaning changes: a reworded English keeps
    its id, and the translations are marked for review.
  - A message whose sense is not plain from its words takes
    `note = "…"` for translators: what it is, where it shows, what each
    argument holds.
- **Plurals are selectors on a count**: English writes `[one]` and
  `*[other]`, and every language writes its own categories. A count that
  never reaches one still uses the selector where another language's
  categories differ (two, few, many).
- **A message that names one thing or counts several** chooses the name
  by the exact `[1]`, never by `[one]`: in Russian `one` also covers 21.
  English writes `[1] Moved { $name } to Trash` and `*[other] Moved
  { $count } items to Trash`; a language adds its categories beside
  `[1]`.
- **Agreement is a selector on a kind**: a message about "the item"
  takes its `Kind`, and English ignores it where its words agree
  already.
- **The part of Fluent Skwir uses:** text, arguments, selectors,
  attributes. No terms, no message referring to another, no functions:
  arguments arrive formatted.

## Arguments

- **A folder named as where something goes** ("to Docs") is a `Folder`:
  unquoted, its controls shown as a `Name`'s are.
- **A file's, a folder's, a place's or a tag's name is a `Name`**,
  never a string placed in the message: it carries the quotes and the
  isolation from the sentence (ADR-139, 5).
- **A name's bidirectional controls are drawn as tags, never obeyed**,
  wherever a name is drawn, in a sentence or alone (a row, a tile, the
  trail, a title, the rename field). The overrides, embeddings and
  isolates (U+202A to U+202E, U+2066 to U+2069) are each a muted tag
  naming it; the marks (U+200E, U+200F, U+061C) are left as they are.
  What is drawn never changes the name: a search, a copy and an
  operation see it as it is.
- **A count is a `Count`, a size a `Size`, a moment an `Instant`, a
  length of time a `Span`**: each formatted for the reader's language
  and the user's settings, never with `format!`, `{:.1}` or `to_string`
  in the code.
- **Words in words are a `Words` argument** (a plan's title in a
  toast). A message never receives another message already said as a
  string.
- **What is not Skwir's to translate** (a path, a command line, a
  regular expression, a query, an identifier, what the system or an
  agent wrote) is `Words::as_written`, or an argument of that kind:
  shown as it is in every language.

## Who reads it

- **Words are said where they are drawn or sent, in the reader's
  language**: the window's for what it draws, English for agents, MCP
  and the CLI's help (`control.language` may change it), and the
  Fluent default's language for logs (English).
- **What is said is kept only while its language is the one said**
  (keyed by the language's revision), and never compared, parsed or
  stored as data: what a later step needs travels as data beside the
  words.

## Words two owners share

- **A submenu several plugins fill, a settings section, a group a
  menu places, a place's kind** is named by an id core owns, which
  carries its words. Two contributions meet by that id, never by their
  words.
- **No label is made from an id, a key or a type name** (capitalised,
  with its underscores replaced): every label has words of its own.

## Nothing reads words back

- **No code takes words apart or matches them**: no `strip_prefix`,
  `contains`, `starts_with` or `==` on what Skwir says, its own or
  another crate's. What decides behaviour is data beside the words (an
  error's kind, a plan's undo, a provider's absence).
- **An error is data with words**, not words: what the caller needs to
  act on (refused, missing, busy, not yet on this system) is its kind.
- **An error's words are `Words`**: `Err(words!(…).into())`, or
  `.context(words!(…))`, a downcast still finding what is inside. What
  shows an error says it with `Words::of_error` or as an `Arg::Error`:
  its chain, link by link, in the reader's language (a link written with
  `words!`, a system's error by its kind, `op::io_words`), any other link
  as it is. Its `Display` is English, for logs, but while a chain is said,
  each link that is words says itself in the reader's language. Never
  format an error with `{:#}` into what the window shows.

## Catalogs

- **Each crate keeps its catalogs in `l10n/`**: `en.ftl`, made from its
  code by `just words` (`tools/words/extract.py`), never edited by hand,
  and one file a language, written by translators. A message of a test
  (in a `tests.rs`, a `tests/` folder, or an id beginning `test_`) is no
  crate's.
- **A plugin's catalogs are a contribution** like its commands:
  registered with its id, withdrawn on disable
  (`crates/skwir/tests/plugins_toggle.rs`).
- **The checks:**
  - `en.ftl` is current with the code;
  - each catalog has the English's ids, arguments and attributes;
  - every selector has its default;
  - no id has two Englishes in one crate;
  - no catalog keeps an id the code no longer says.

## Tests

- **A test of what Skwir says checks it in English**, or checks the
  message's id and arguments; never a translation.
- **A screen reader's names and agents' texts are tested like the
  window's.**
- **The pseudo-language** (accented, longer, every argument marked) runs
  in the real-window checks: English left on a surface, cut text and an
  argument placed wrong show there.

## What stays English

- identifiers, settings' keys and values, the query language's words
  and units (ADR-139, 9 and 11), the keymap's notation;
- logs and the helpers' messages to Skwir;
- a file's name, "(copy)" included (ADR-054).
