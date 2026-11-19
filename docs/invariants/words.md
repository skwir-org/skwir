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
  - One exception, the user's choice (2026-11-17): words written to be
    said inside sentences (a plan's title, a step's tenses: "copy 3 items
    to Docs") that begin what is shown (a toast, a row) are
    `Words::heading()`. Their first letter is cased as the reader's
    language begins a sentence (ICU4X: a Turkish i takes its dot, a
    script without case is left as it is). Nothing else is cased in
    code.
  - A list of items is a `List` argument (`List::and`, `List::or`):
    joined by the language's patterns ("a, b et c").
  - A link inside a sentence is drawn from `Words::say_around(name)`:
    what comes before the argument and after it, the language placing
    it; several arguments drawn apart (a line of facts wrapping only
    between them) from `Words::say_apart(names)`. Never cut said text
    in the caller.
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
  already. A message naming one of Skwir's places agrees with the
  place's own gender (below, "Nouns").
- **A noun of Skwir's is marked `noun`**: a title that sentences name as
  an argument (a place's: Home, Trash, Recent, Shelf, Nearby, Root),
  `words!(trash_title = "Trash", noun, note = "…")`. It takes no
  argument. `just words` writes `# noun` above it in `en.ftl`, which lets
  its translations give it their grammar (below).
- **The part of Fluent Skwir uses:** text, arguments, selectors; a
  noun's attributes in a translation, and two functions a translation
  calls on an argument, `GENDER` and `FORM` (below). No terms, no message
  referring to another, no other function: arguments arrive formatted.
  English writes no attribute and calls no function.

## Arguments

- **A folder named as where something goes** ("to Docs") is a `Folder`:
  unquoted, its controls shown as a `Name`'s are.
- **A path is a `Path`** (`Arg::Path`, from a `&Path`), never text: it is
  made of names, so it is said as a `Folder` is, unquoted, its controls
  shown, isolated where the window draws it.
- **A file's, a folder's, a place's or a tag's name is a `Name`**,
  never a string placed in the message: it carries the quotes and the
  isolation from the sentence (ADR-139, 5). The quotes are its
  language's data, core's `name_quote_open` and `name_quote_close` said
  in the sentence's language (English's where its catalog has none):
  never a table of languages in code, never quotes written around a
  name's argument in a message.
- **A place named in a sentence is given as `Words`**, its title as the
  place has it (`App::location_title`, `Place::title`), never said into a
  string first: `Folder(title.say_plain())` loses the noun, and with it
  the gender and forms a language takes it in.
- **A name's bidirectional controls are drawn as tags, never obeyed**,
  wherever a name is drawn, in a sentence or alone (a row, a tile, the
  trail, a title, the rename field). A name drawn alone is
  `Words::name_alone` (a tab's, a crumb's, a title's), or, where it is
  drawn as a string, `words::shown_controls`: its controls shown in every
  saying, isolated where the window draws it, nothing else added (no
  quotes, no case); as data, the name as it is. The overrides, embeddings and
  isolates (U+202A to U+202E, U+2066 to U+2069) are each a muted tag
  naming it; the marks (U+200E, U+200F, U+061C) are left as they are.
  What is drawn never changes the name: a search, a copy and an
  operation see it as it is.
- **Where the window draws it, a name is isolated from its sentence**
  (`Words::say`): FSI (U+2068) before it, PDI (U+2069) after, inside its
  quotes ("“FSI name PDI”"), and a `Folder`'s alike, bare. The quotes are
  the sentence's punctuation and open and close in its direction; the
  name takes its own from its first strong letter (UAX #9), so a Hebrew
  name, or one ending in digits or a bracket, reorders nothing around it.
  The name holds no control that could end its isolate early: its own are
  tags (above). Fluent's own isolation stays off (ADR-139, 3).
  - **The marks never go** into English (`english()`: agents, MCP, the
    CLI's help, logs, tests), words said in given languages (`say_in`),
    data (`to_data`, `Serialize`), nor anything said for the window that
    leaves it as text: the clipboard, a file's name, a name kept as data
    (a saved search's), a command's argument, a measure of what is read.
    Those say `Words::say_plain()`: the window's language, no mark.
  - **What assistive technology reads is said plainly too**: an
    accessible name, description, value or announcement carries no mark
    (speech needs no isolation, and a braille display shows a mark as a
    stray cell). gpui leaves the isolates out of every node it hands
    AccessKit (patch 0077), a text's runs apart, whose characters the
    caret counts (`accessibility.md`).
  - **Words said for the window never become a `Name` or a `Folder`**
    (a place's title in "Could not read “…”"), nor text as written (a
    search's title made of words, `Words::as_written`, which agents read
    as it is): they are said with `say_plain()` there, and isolated once,
    by the words they go in. Said with `say()`, their marks would show as
    tags, and reach agents in `location_title`'s English.
  - **What is printed or written to a file is plain** (a PDF's text and
    name, an editor's buffer, a keyring's label, a metadata value): no
    mark reaches a document, a program or the disk.
  - **A sentence cut around an argument** (`say_around`, `say_apart`)
    hands back parts holding whole isolates only: the argument cut out
    leaves no mark (the caller draws it, an element of its own that
    nothing beside it reorders), and a name its quotes.
- **Keys a shortcut is pressed with are a `Keys`** (`Ctrl+Shift+C`, as
  the window names them: `keymap::display_keys`, `Shell::keys_for`),
  never a string placed in the message: where the window draws them, they
  are isolated left to right (LRI U+2066 before them, PDI U+2069 after),
  so a sentence written right to left never reorders their strokes, and
  the sentence's punctuation around them stays its own. Plain everywhere
  the marks never go (above: English, `say_in`, `say_plain`, data, which
  brings them back as keys). Keys drawn alone, beside what they run (a
  menu's row, the palette's), are made left to right where they are drawn
  (`sides::shortcut`).
- **A count is a count, a size a `Size`, a number with digits after the
  point a `Decimal`, a moment a `Moment`** (its `Shape`: a time, a date,
  a month…): each formatted for the system's number or date locale and
  the user's settings, never with `format!`, `{:.1}`, `strftime` or
  `to_string` in the code. A length of time is counts in a message.
  A count said short (in the room of a bar: 1.2k, 1.2万) is in the powers
  of ten the language its words are said in counts in, as CLDR's
  compact decimals say them (`locale::compact_power` of
  `locale::said_in`: thousands in English, myriads in Japanese, lakhs in
  Hindi, none below millions in German); each power's unit is a variant
  of its message, never CLDR's text. Which settings: core's `locale` holds them for every owner alike, the
  dates setting's ISO 8601 choice (`set_iso_dates`) and the desktop's own
  12- or 24-hour switch (`set_hours`), so a plugin's dates follow them
  as the window's do.
- **A number and its unit stay on one line**: the message joins them with
  a no-break space, `\u{a0}` in the English (`{ $n }\u{a0}MB`, `about
  { $minutes }\u{a0}min left`), and a dimension's numbers to their sign
  (`{ $width }\u{a0}×\u{a0}{ $height }`); gpui wraps at a plain space
  only. `just words --check` refuses an English that parts them, and
  the catalogs' test a translation that parts an argument its English
  joins. A unit written as a word ("pixels") is prose, which may break.
  English writes a percent sign right after its number, with no space
  (`{ $n }%`, "100%"); a language that spaces it joins it (French
  « 42 % », a narrow no-break space).
- **Words in words are a `Words` argument** (a plan's title in a
  toast). A message never receives another message already said as a
  string.
- **What is not Skwir's to translate** (a command line, a regular
  expression, a query, an identifier, what the system or an agent wrote)
  is `Words::as_written`, or an argument of that kind: shown as it is in
  every language. A path is a `Path` (above).

## Nouns

- **A noun's translation gives it its grammar**, as attributes, in a
  language that declares one (`tools/words/grammar`): its gender,
  `.gender = feminine`, and each form the language declares, written
  whole (`.dative = dem Papierkorb`; French `.definite = la Corbeille`,
  `.de = de la Corbeille`). Every noun of the language is given every
  one: a sentence may ask for any.
- **A sentence chooses by an argument's gender** with
  `{ GENDER($place) -> [feminine] … *[other] … }`, and **places an
  argument in a form** with `{ FORM($place, case: "dative") }`: exactly
  these two, valid Fluent, read by the resolver (`pattern::from_fluent`)
  in the language the sentence is said in.
  - A noun's attribute is read from the catalog that says the noun, while
    that is the sentence's language: one language written one way
    (`locale::same_writing`: `fr-CA` and `fr`, `zh-Hant` and `zh-HK`). A noun
    said in English, or in another of the reader's languages, has none
    there.
  - What is no noun (a file's name, a path, text as written, a folder
    the user named) has no gender, `other`, and is its own form, as it is
    said (a name quoted). So every `GENDER` defaults to `*[other]`, which
    says what such an argument takes.
  - A form a name has not (one fusing a preposition with the noun's
    article, French « du »: `de+` in the declaration) is taken only in a
    `GENDER` variant other than `*[other]`, where the argument is a noun;
    `*[other]` says what a name takes (« de Docs »).
- **A language declares its grammar once**, a line of
  `tools/words/grammar`: its genders and its forms, named as CLDR's
  grammatical features name them (masculine, feminine, neuter, common;
  nominative, accusative, genitive, dative, locative, instrumental…;
  definite, the noun with its article), and nothing else. The program
  reads nothing of it: it is what the catalogs' test holds a catalog to.
- **A language that declares none** gives its nouns nothing, and its
  sentences choose by no gender: they say a place as it is.

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
- **A plugin from outside brings its own** (`l10n/` beside its manifest,
  its `en.ftl` written by its author): never compiled into Skwir nor
  counted in a language's share (`just words` leaves
  `crates/plugins-outside/` out), registered under `outside:<id>` while
  it is on and withdrawn when it is turned off (ADR-139, 14).
- **What describes a plugin is data about it, said on or off**: its
  title, what it does, what it needs a program for, on the Plugins page
  where a reader decides to turn it on. Core makes these words
  (`PluginHost::list`, `probe_requirements`: `Words::describing`, which
  nothing else can call), said from the plugin's catalogs registered while
  it is on, and while it is off from the catalogs Skwir ships for its
  crate, or those its host keeps for a plugin from outside, read when it
  is found (`words::describe_with`). Those are never registered: a plugin
  that is off leaves no trace, and nothing else it would say (its
  commands, its answers) is said from them. Read a message at a time, no
  index made; as data, describing words are the words they wrap, and
  come back describing nothing.
- **A reader's language finds a catalog by its script, then its region**
  (`locale::matching`, ICU4X's likely subtags of both): a catalog of its
  language written in its script, a catalog named by its language alone
  being in the script that language is most likely written in (`zh` is
  Simplified Chinese, `sr` Cyrillic Serbian). So `zh-TW`, `zh-HK` and
  `zh-MO` find `zh-Hant`, `zh-CN` and `zh-SG` find `zh-Hans` or `zh`, and
  `sr-Latn` never `sr`. Among those, its region's first, then one naming
  none (`fr-CA` finds `fr`), then another region's (`pt-BR` finds
  `pt-PT` only after `pt-BR` and `pt`). The code's English is the catalog
  `en`, so an `en-GB` one comes first for a British reader. What the
  system translates (a desktop entry's `Name[zh_TW]`, the type database's
  `xml:lang`) is chosen the same way (`locale::Ranker`).
- **When a language is offered** (ADR-139, 13, amended 2026-11-18):
  Automatic speaks a language only once `tools/words/offered` names it,
  reviewed or as a draft (`zh-Hans draft`), and Skwir's own words are
  said in a system language only then (`locale::languages`). Any other
  language a catalog is in is listed by the Language setting as
  incomplete, with its share ("Français — incomplete, 99% translated, 0%
  reviewed"), and said only to whoever chooses it. What the system
  translates itself (desktop entries' names, the type database's
  comments) is read in the user's languages whatever Skwir offers
  (`locale::preferred`), a pseudo-language none of them.
  - A message is translated once its language's catalog has its id, and
    reviewed once the `# draft` comment above it is gone: a model's or a
    machine's translation is a draft until a person who speaks the
    language removes that line.
  - `just words` counts each language's share into
    `crates/skwir/src/catalogs.rs` (`words::Share`, its `Offer`, and its
    name in itself, core's `settings_language_self`), so the program reads
    no catalog to know it, nor to list the languages (Settings, About); a catalog edited is followed by
    `just words`. It writes a language into the record as a draft once
    90% of all messages are translated in it and every message of what
    is on by default is: the crates that are no plugin's, and each plugin
    whose `enabled_by_default` is not the literal `false`. It takes the
    `draft` away once as much is reviewed.
  - **A draft spoken says it is one** (`locale::draft_spoken`): the
    first start Automatic speaks it, a toast with a button for English,
    once a language (`drafted.rs`, kept in the state file as
    `ui.drafts_told`); About's language line while the window speaks
    one, chosen or not; the Language setting's "draft, 3% reviewed". The
    words say a draft, never who made it: the catalog does not know.
  - Only a person takes a language out of the record, which lists it as
    incomplete again. Between releases it stays offered, what it lacks
    said in the next of the reader's languages; a release refuses one
    offered under 75% reviewed, and one offered as a draft short of the
    draft's bar (`just words --release`, run by `lint-all`): a draft's
    new messages are drafted before a release.
- **The checks:**
  - `en.ftl` is current with the code;
  - each catalog has the English's ids and arguments;
  - every selector has its default;
  - attributes only on a noun, each its language's (`tools/words/grammar`:
    a gender it declares, a form it declares, text alone), every noun of
    a language that declares a grammar given each; English gives none;
  - `GENDER` and `FORM` only by the language's genders and forms, on an
    argument the English gives that is no count or kind it chooses by;
    every `GENDER` defaulting to `*[other]`; a form a name has not only in
    a `GENDER` variant on the same argument other than `*[other]`. The
    checks cannot see an argument's type: a `GENDER` on a name is not
    refused, and says `*[other]`;
  - no id has two Englishes in one crate;
  - no English writes quotes around an argument (`“{ $path }”`): it is a
    `Name`, a `Folder` or a `Path`, or said unquoted (`just words
    --check`; those written before the check are listed in
    `tools/words/quoted`, which only shrinks);
  - no catalog keeps an id the code no longer says;
  - the languages' shares counted and the record current; for a
    release, no language offered under 75% reviewed.

## Tests

- **A test of what Skwir says checks it in English**, or checks the
  message's id and arguments; never a translation.
- **A screen reader's names are tested as it reads them**: in the
  window's language with no isolation mark (`say_plain`'s text);
  **agents' texts** in English.
- **The pseudo-language** (accented, longer, every argument marked) runs
  in the real-window checks: English left on a surface, cut text and an
  argument placed wrong show there. Its right-to-left twin (CLDR's
  `ar-XB`, `direction::pseudo_rtl`) is said the same way, the English
  with its arguments marked, drawn right to left: one hook says both
  (`pseudo_of`). To what a locale decides, both are English
  (`locale::is_pseudo`): no catalog but English's, no system translation
  (`ar-XB` never Arabic), English's numbers, dates, order and script.
- **A language's grammar is tested on a catalog of the test's own**
  (German's cases, French's articles), never on a shipped translation,
  which its reviewers change.

## What stays English

- identifiers, settings' keys and values, the query language's words
  and units (ADR-139, 9 and 11), the keymap's notation;
- logs and the helpers' messages to Skwir;
- a file's name, "(copy)" included (ADR-054).
