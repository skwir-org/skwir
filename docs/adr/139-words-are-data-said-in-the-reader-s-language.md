# ADR-139: Words are data, said in the reader's language

Status: accepted, 2026-11-16, on the user's answers. They asked how
Skwir could be translated, read the research (`docs/i18n.md`), and
chose what was recommended: Fluent's format (3), English written beside
an id in the code (2), agents kept in English (7). For sizes they asked
for a setting, 1000-based by default and 1024-based as the other choice
(9).

Settled the same day, on their answers to six more questions, each the
one recommended:
- a name's controls shown as tags (5);
- French drafted, then reviewed by them; Weblate after the conversion;
  when a language is offered (13);
- name order by the locale (15).

For user folders' names (16) they asked for the system's way by
default, a setting to keep the names on disk, and a check of Linux,
where Dolphin shows "Dossier personnel".

Designed; nothing is built. The steps are PLAN.md's "Words in every
language".

Amended 2026-11-18, on the user's choice of the design recommended to
them: Skwir's nouns (a place's title) carry their gender and forms in
each language's catalog, as attributes a sentence naming them reads with
two functions, `GENDER` and `FORM` (3, 5); the quotes around a name are
each language's catalog data, not a table in the code (5).

Amended again 2026-11-18, on the user's choice: Automatic speaks a
translation once it is translated, before it is reviewed, and the window
says it is a draft (13).

## Context

- **Skwir speaks English only, and its English is finished text
  wherever it goes.**
  - About 8,000 literals, counted by a lexer (2026-11-16; occurrences,
    not messages): `crates/ui` 2,700, the plugins 3,300, core 1,050,
    platform 630.
  - A command's title and description are `&str` at registration, and
    the same strings are its palette row, its F1 heading and its MCP
    tool (`crates/plugins/control`).
  - A plan's steps carry `op::Words` already written in English, sent to
    agents inside `PlanData`; a field's reference (ADR-137) is English
    items agents read.
  - A plugin from outside says its titles in plain strings, and protocol
    1 tells it no language.
- **Some code reads its own English back.** The toast takes `"undo "`
  off a title and `"{name} could not "` off an error
  (`crates/ui/src/toast.rs`); the placeholder looks for `"no provider"`
  and `"permission denied"` (`crates/ui/src/placeholder.rs`).
  Independent plugins meet in one submenu by its English title
  (`"Compress"` from archives and shrink, `"Open With"`, `"Tags"`), and
  settings sections by their heading. Translated by two people, one
  submenu would become two.
- **Sentences are built with English grammar**: a plural made by adding
  `s` (`format::count`), fragments slotted into templates
  (`system::not_yet`), lowercase titles capitalised in code, English
  quotes around 673 names, lists joined with `", "` and `" and "`, dates
  from strftime patterns holding English words, sizes divided by 1024
  and labelled KB. French and German make words agree with a noun's
  gender: "1 dossier sélectionné", "1 image sélectionnée".
- **A sentence that names one of Skwir's places needs the place's
  grammar** (amended 2026-11-18): "Moved to { $place }" is « dans la
  Corbeille » but « dans Récents » in French, „in den Papierkorb“ but „in
  die Bibliothek“ in German, and Czech, Polish, Russian, Arabic and
  Finnish take a case too. Until then only a `Kind` (file, folder, link,
  item) gave a message something to agree with, and the English worked
  around it a message per place. Fluent's own answer is a term's
  attributes and parametrised terms (`-brand-name.gender`,
  `{ -brand-name(case: "locative") }`), which Skwir's catalogs, made of
  messages only, do not have.
- **Some text already comes in the user's language** from the system
  (desktop entries' names, macOS's and Windows' type names, GVfs's and
  KDE's dialogs), so a translated system shows a window of two
  languages today.
- **Measured** (2026-11-16, `docs/STATUS-LOG.md`):
  - **fluent-bundle 0.16 holding a catalog of 8,000 messages** (575 KiB
    of FTL) takes 6.2 MiB of heap and 3 ms to parse. A translated window
    needs English beside it for what is missing: about 12 MiB, against
    the 17 MiB between the last idle reading (83.2 MiB) and the budget
    (100 MiB, ADR-033).
  - **Indexed instead, each message parsed when first said:** 190 KiB
    and 0.8 ms for the index, about 1.1 KB and 0.7 µs a message said.
  - **Saying a message** takes 8 ns without arguments, 75 ns with one,
    280 ns with a plural.
- **The lighter Fluent loaders, read the same day:**
  - **fluent-zero 0.2.0** returns a message without arguments as a
    `&'static str`. On the first message with one, it parses the
    locale's whole FTL into a fluent-bundle, the 6.2 MiB above, at
    Skwir's first frame (the footer's counts). English is a catalog too.
  - **fluent-static 0.6.0** compiles every message of every language to
    Rust code. Its build refuses a message one language lacks, so a
    translation in progress cannot ship. Its arguments are positional,
    in the English's order, and nothing loads at run time.
  - **Both say a message where it is called**, into a string, so
    neither carries it as data to be said later (1).
- The rest of the research (libraries, other projects, translators'
  platforms, formats, scripts) is `docs/i18n.md`.

## Decision

1. **What Skwir says is data: `Words`** (`skwir_core::words`).
   - It holds who says it (its crate: the domain), which message (its
     id) and its arguments, typed (5). Text Skwir did not write (an
     agent's title, the system's message, a plugin from outside on
     protocol 1) is held as written (`Words::as_written`).
   - `Words` is serializable and has a schema, so it travels where
     English travels today: commands' titles and descriptions, settings'
     texts, menu items, places, plans' titles and their steps' words,
     toasts, errors, references (ADR-137), accessible names, the plugin
     protocol (14).
   - The functions that register or show text take `Words`: an English
     literal there does not compile, once the crates are converted (a
     transitional `From<&str>` goes with the last).
   - Words are said (resolved) where they are drawn or sent, in the
     language of who reads them, never earlier. What one says is for
     that reader: kept only while the language is the one it was said
     in, never compared or parsed.

2. **Written as English beside an id** (the user's choice):

   ```rust
   toast(words!(trash_moved = "Moved { $count ->
       [one] one item
      *[other] { $count } items
   } to the Trash", count = n))
   ```

   - **The macro checks at compile time** that the English is a message
     in the part of Fluent Skwir uses (3), and that every argument it
     names is given, with its type, and every one given is named.
   - **The id is the crate's**: `snake_case`, unique in the crate,
     changed only when the meaning changes. A fixed typo keeps it, and
     the translations are flagged for review, not lost.
   - **A note for translators** is the macro's `note = "…"`.
   - **Each crate's English catalog** (`l10n/en.ftl`) is made from its
     code by a tool reading the macros, never written by hand. A check
     fails where it is out of date, where an id has two Englishes, or
     where a catalog keeps an id the code no longer says.

3. **Translations are Fluent files** (the user's choice):
   `l10n/<language>.ftl` in each crate, its messages by the English's
   ids.
   - **The part of Fluent Skwir uses:** text, arguments, selectors on a
     plural category or on an argument's word (a kind); and a noun's
     grammar (amended 2026-11-18, 5): a translation of a noun of Skwir's
     gives it attributes, its gender and forms (`.gender = feminine`,
     `.dative = dem Papierkorb`), and a sentence naming one chooses by its
     gender, `{ GENDER($place) -> [feminine] … *[other] … }`, or places it
     in a form, `{ FORM($place, case: "dative") }`. Both are written in
     Fluent's function syntax, so a catalog stays Fluent that any of its
     tools (Weblate's) reads. No terms, no message referring to another,
     no other function: arguments arrive formatted (5).
   - **Isolation marks are off**: a name isolates itself (5).
   - **A check holds every catalog to its English**: the same ids, the
     same arguments, every selector with its default, and Fluent's
     syntax; attributes on a noun only, each its language's (5), every
     noun given each of them; `GENDER` and `FORM` only by the language's
     genders and forms, on an argument the English gives that is no
     count or kind, every `GENDER` with `*[other]` its default.

4. **Skwir says them itself**, with a resolver of its own over
   `fluent-syntax`'s parser: none of the runtimes read fits (context).
   - **Each language's catalogs are read-only data in the binary**
     (`include_str!`), so pages never read are never resident. A catalog
     is indexed the first time a message of it is said.
   - **A message without arguments is the catalog's own text**, returned
     as it lies there, as fluent-zero does.
   - **A message with arguments is parsed when first said** and kept
     while its language is the one said.
   - **English is never a catalog**: the macro turns the English into a
     ready structure at build time, as fluent-static does with every
     message, so English is never parsed at run time.
   - **Plural categories come from ICU4X** (`icu_plurals`, CLDR's
     current rules), not from Fluent's crates (CLDR 37).
   - **A message missing in a language is said in the next of the
     reader's languages**, English last, and always whole: never half a
     sentence in one language and half in another.
   - **A noun's gender and forms are read where a sentence names it**
     (amended 2026-11-18): from the catalog that says the noun, while it
     is the sentence's language (a tag with more or fewer subtags
     included). A noun said in English, or in another of the reader's
     languages, has none there, as a name has none.
   - **If the idle reading or the first frame shows the cost**, the next
     step is translations compiled into indexed tables at build time, as
     Slint does.

5. **Arguments are typed, and formatted before the message reads them.**
   - **A name** (`Name`: a file's, a place's, a tag's) is quoted as its
     language quotes ("…", « … », „…“) and isolated from the sentence
     around it where a window draws it. The quotes are each language's
     data (amended 2026-11-18): two of core's messages,
     `name_quote_open` and `name_quote_close` (“ and ” in English), so a
     new language quotes as it does without code, and Simplified and
     Traditional Chinese, their catalogs found by script, quote apart
     (“…” and 「…」).
   - **A noun of Skwir's** (amended 2026-11-18, the user's choice: a
     place's title, Home, Trash, Recent, Shelf, Nearby) is marked so where
     it is written (`words!(trash_title = "Trash", noun)`), takes no
     argument, and is given to sentences as `Words`.
     - Each language's catalog gives it its grammar as attributes: its
       gender (`.gender = feminine`) and every form the language's
       sentences take a noun in (`.dative = dem Papierkorb`, French
       `.definite = la Corbeille` and `.de = de la Corbeille`).
     - Which genders and forms a language uses it declares once, in
       `tools/words/grammar`, named as CLDR's grammatical features name
       them (masculine, feminine, neuter; nominative, accusative,
       dative…; definite), a form fusing a preposition with the noun's
       article (French « du », de and le) marked as one a name has not.
       English declares none and writes no attribute.
     - What is no noun (a file's name, a path, text as written, a folder
       the user named) has no gender, `other`, and is its own form, so a
       sentence written for nouns still says a name right: « dans
       { FORM($place, case: "definite") } » is « dans la Corbeille » and
       « dans Docs ». A form a name has not is taken only in a `GENDER`
       variant other than `*[other]`, which says what a name takes.
     - Nothing of it is code: a language's grammar is its catalogs' and
       its line in `tools/words/grammar`.
   - **A path** (`Path`) is made of names: said as a folder is, unquoted,
     its controls shown, isolated from the sentence where a window draws
     it.
   - **A name's bidirectional controls are shown, never obeyed** (the
     user's choice). The overrides, embeddings and isolates (U+202A to
     U+202E, U+2066 to U+2069) are each drawn as a small muted tag
     naming it (RLO), wherever a name is drawn, in a sentence or alone
     (`Words::name_alone`: a tab's, a crumb's, a title's):
     - so `Hello_` + U+202E + `gnp.js` reads in its real order, not as
       `Hello_sj.png`;
     - in the rename field the tag is one character the caret passes
       and Backspace removes.

     The marks (U+200E, U+200F, U+061C) are left as they are: Arabic and
     Hebrew names use them, and they reverse no letters. A search matches
     the name as it is.
   - **A count** (`Count`) chooses the plural and is written with the
     locale's digits and groups.
   - **A kind** (`Kind`: file, folder, link, picture…) lets a language
     make its words agree; a noun's gender lets it agree with one of
     Skwir's places.
   - **A size** (`Size`) is written as the setting says (9).
   - **A moment and a span of time** (`Instant`, `Span`) are written by
     ICU4X in the locale's words, on the days of
     `skwir_platform::calendar` (ADR-017).
   - **Words may hold Words** (a plan's title inside a toast).
   - **Nothing is capitalised, joined or pluralised in code**: a
     sentence is a message, and so is a list's pattern.

6. **The language is the user's**, read by `skwir_platform::locale`
   (beside `xdg` and `calendar`).
   - **On Linux, gettext's rules apply.** `LANGUAGE` comes first, but
     is ignored under the C locale; then come `LC_ALL`, `LC_MESSAGES`
     and `LANG`. `C`, `C.UTF-8` and `POSIX` mean English. Modifiers such
     as `@latin` are mapped to BCP 47. Formats come from `LC_TIME` and
     `LC_NUMERIC`, apart from messages; C there writes them as English
     does.
   - **On macOS and Windows:** the preferred languages and the region's
     formats on macOS; the display languages and the user's locale on
     Windows. Their names are BCP 47 tags already, read whole: a script
     they name stays (`zh-Hant-TW`, `sr-Latn-RS`), so a reader of
     Traditional Chinese finds no Simplified catalog.
   - **`core.language`** is Automatic (the system's) or a list chosen
     in Settings, which a Windows user whose language is not an
     installed display language needs.
   - **A change shows at once**, as a font's does (ADR-073): the windows
     draw again in the new language. What the system fixes as a program
     starts (the fonts' fallback language, macOS's bundle language)
     follows at the next start, and Settings says so.

7. **Agents read English** (the user's choice).
   - A command's id is its contract. Its title, description and schema
     are said in English to MCP, to the CLI's help and to scripts, and
     so is display text inside a JSON answer (a facet's "1 file"). MCP
     clients keep tool lists, and a description that moved with the
     window's language would change an agent's prompt beneath it.
   - `control.language` may give agents the window's language instead.
   - What the user reads is said in theirs: the permission question, a
     plan's review, its toasts, an agent's plan included. An agent's own
     words (a title it wrote) are shown as written.

8. **Words two owners share have one owner, by id.**
   - A submenu several plugins fill (Compress, Open With, Tags) and a
     settings section are named by an id core owns, with its words,
     never matched by title.
   - No code reads words back: what it needs to know travels beside
     them as data (an undo's plan, an error's kind, a missing provider).
   - No label is made from an id (a group's, a key's, a command's
     fallback): every one has words.

9. **Sizes follow a setting** (the user's choice): `core.size_units`.
   - **Decimal by default:** 1000 bytes a kilobyte (kB, MB, GB), as
     macOS's Finder and GNOME's Files show and as volumes are sold.
     **Binary:** 1024 (KiB, MiB, GiB), as KDE's Dolphin shows.
   - **Every size Skwir shows follows it**: files, folders, volumes,
     speeds, limits.
   - **The query language does not move with it.** `k`, `M` and `G`
     alone stay 1024, as saved searches hold them; `kB`, `MB` and `GB`
     read 1000, and `KiB`, `MiB` and `GiB` read 1024. What Skwir writes
     into a query (a size group's term, the reference's examples) uses
     the unit the setting shows.

10. **Formats come from ICU4X, adjusted by the system.**
    - Numbers (`icu_decimal`), lists (`icu_list`) and plurals (4).
    - Dates and times (`icu_datetime`), with data only for English and
      the languages Skwir ships, cut by ICU4X's data generator.
    - The hour cycle and the first weekday the system says, passed as
      the locale's extensions.
    - `ui.date_format`'s ISO choice stays Skwir's own, and words such as
      "Today" and "Yesterday" are messages.

11. **The query language stays one language.** `type:`, `kind:`,
    `modified>=`, `today` and `this-week` are its words in every
    language, as saved searches and places hold them (ADR-109): Windows
    Search found that keywords in the window's language break a query
    written in another. What shows them (pills, completions, the
    reference, size and date groups) says them in the user's language
    beside the term.

12. **Left to right first.** gpui lays out no mirrored window and draws
    some right-to-left text wrong (`docs/i18n.md`), so a language written
    right to left (Arabic, Hebrew, Persian) waits for a track of its
    own. A name, isolated (5), is drawn right in any language.
    Right-to-left text itself (level 1: laid out, wrapped and edited
    right inside a left-to-right window) is built (amended 2026-11-17,
    gpui patches 0069 to 0071), and the mirrored window (amended
    2026-11-18, 0075: the window laid out from its right while it
    speaks a right-to-left language, documents and media's controls left
    to right, a document's paragraph in its own text's direction, 0081).
    What waits is a real right-to-left catalog and its review.

13. **Shipping.**
    - **Catalogs are compiled into the program**; a plugin from outside
      brings its own.
    - **The desktop file and the AppStream metainfo** are translated
      through gettext's PO, merged when packaged (`msgfmt --desktop`,
      `--xml`).
    - **macOS's bundle declares the languages offered**, reviewed or as
      a draft (`CFBundleLocalizations`, one `.lproj` each), so its own
      menus and the per-app language follow. A language shipped but not
      offered is left out (amended 2026-11-17): Automatic never speaks
      it, and a Mac in that language would mix macOS's menus in it with
      Skwir's English. The Windows installer stays English.
    - **When a language is offered** (the user's choice, amended
      2026-11-18). Automatic speaks a language as a draft once 90 % of
      its messages are translated, and every message of what is on by
      default (core, the window, the plugins on by default) is: for most
      people whose system speaks it, a translation not yet reviewed
      serves better than English, and a setting to find in a language
      they may read poorly is the worst first impression. It offers it
      reviewed once as much is reviewed. Below the first bar, Settings
      lists it as incomplete with its share, for whoever chooses it, and
      Automatic never picks it: a window half in English is worse than
      either. For a release, a language offered reviewed must keep 75 %
      reviewed, and one offered as a draft the draft's bar (its new
      messages drafted first), or it goes back to incomplete.
    - **A draft says it is one** (amended 2026-11-18): the first start
      on which Automatic speaks a draft, a toast says "This translation
      is a draft, not fully reviewed yet." with a button for English;
      About's language line says it while the window speaks one, chosen
      or not; Settings lists it as a draft with its share reviewed. The
      words say a draft, not who made it: the catalog records whether a
      message is reviewed, not whether a model or a person wrote it, and
      the reader's question is whether anyone checked it. The notice is
      in the window, not only on the website: whoever updates through a
      package never visits it, the browser's language is not the
      system's, and only the window offers English where the drafted
      words are.
    - **A machine's or a model's translation is a suggestion** until a
      person who speaks the language has accepted it (ADR-000). A draft
      is kept marked as one, and counts as reviewed only once accepted;
      spoken before then, it says it is one (above).
    - **French first, drafted by a model and reviewed by the user** (the
      user's choice). The draft follows a glossary settled with them
      first: Skwir's own terms (entry, place, pane, trail, shelf, lens,
      plan, Activity) and French typography (« » and U+202F before
      `; : ! ?`). The review is done in the window running in French,
      surface by surface, with a list of the messages that have plurals
      and kinds.
    - **Simplified Chinese next, drafted by a model** (2026-11-18, the
      user's choice, for issue #13), from a glossary of the terms
      Chinese file managers use (`docs/glossary-zh-Hans.md`), for a
      speaker of Chinese to review.
    - **Translators work on Hosted Weblate** (its Libre plan; Skwir is
      Apache-2.0 and public), opened once every crate is converted
      (PLAN.md's step E). Before that, ids still move, and on Weblate a
      renamed id loses its translations. On hold since 2026-11-18: the
      Libre plan is granted to a project three months after its first
      commit and with contributors; until then a translation comes as a
      pull request of `l10n/<language>.ftl` files (the user's choice).

14. **Plugins from outside get protocol 2**, a track of its own.
    `InitializeParams` carries the languages; the manifest's titles
    become ids into the plugin's own `l10n/`; an answer may carry
    `{ id, args }`, which the host says from that catalog. A plugin on
    protocol 1 is shown as it writes. Built 2026-11-18 (ADR-045, 046,
    047 amended): a module asks its `languages` import; a plugin's
    catalogs are untrusted input, bounded, registered under
    `outside:<id>` while it is on; what describes a plugin (its title,
    what it does, what it needs a program for) is said in the reader's
    language while it is off too: from the catalogs shipped for its
    crate, or those its host keeps from when it found it, never
    registered (amended 2026-11-18).

15. **Names are ordered by the locale's collation** (the user's choice;
    a later track, measured).
    - **The locale** is the formats' (`LC_COLLATE`, the region), not the
      window's language.
    - **ICU4X's numeric sort keys** replace `collate`'s keys behind the
      same seam: made on workers, as orders are (ADR-120, ADR-121), and
      held in memory only, since a new ICU4X may change their bytes.
    - **The C and POSIX locales** keep today's order.
    - **Gate:** a bench case on `flat100k` against today's keys, within
      budget; ICU4X's collator adds about 1.2 MiB to the binary.
    - **Type-to-find's prefix** follows the same collation.

16. **User folders show the names their system shows** (the user's
    choice; a later track).
    - **Windows and macOS:** a folder the system names (desktop.ini's
      `LocalizedResourceName`, a `.localized` folder) shows the name
      Explorer or Finder shows, wherever it is shown as a name: rows,
      tiles, the sidebar, the trail, a title.
      - Its name on disk is kept wherever a path is written, edited,
        copied or renamed, and in Properties.
      - A search matches both names.
    - **A setting, `core.folder_names`,** shows the names on disk
      instead.
    - **On Linux** there is nothing to show apart: `xdg-user-dirs`
      makes the user folders in the user's language on disk
      (`~/Images`, `~/Bureau`). There the setting is shown unavailable,
      saying why.
    - **What Linux's file managers translate is the places' titles.**
      KDE keeps Home, Network, Trash and Recent Files in English in
      `user-places.xbel` (`isSystemItem`), and Dolphin says them in the
      user's language ("Dossier personnel"). Skwir's own places' titles
      (Home, Trash, Recent, Network, Nearby) are its words in every
      system, said in the user's language (1).

## Consequences

- **Every crate changes.** About 8,000 literals become `words!`, crate
  by crate (PLAN.md), the English unchanged until a language is added.
- **Tests compare English as it is said in English.** The settings
  wording test (ADR-072) holds the English.
- **Core gains** `words` (the type, the resolver and the catalogs'
  registry, a plugin's withdrawn on disable), a proc-macro crate,
  `fluent-syntax`, and ICU4X's `icu_plurals`, `icu_decimal`, `icu_list`
  and `icu_datetime` with data for the languages shipped. Each is
  measured against `just idle-pss` and the first frame as it lands.
- **A sentence naming a place reads its noun's attributes** (amended
  2026-11-18), each parsed from its catalog when first asked and kept
  beside the catalog's messages, as a message is. English reads none.
- **The checks cannot see an argument's type** (amended 2026-11-18):
  the catalogs' test refuses `GENDER` and `FORM` on what the English
  chooses by (a count, a kind), and the resolver treats any argument
  that is no noun as having no gender and being its own form, so a
  `GENDER` on a name is never wrong, only useless.
- **Amended:**
  - **ADR-099:** a shared submenu is named by core's id (8).
  - **ADR-037:** on Linux a type's name is read in the user's language
    from shared-mime-info's translated comments, a step of its own.
  - **ADR-045 and ADR-046:** protocol 2 (14).
  - **ADR-054 stands:** "(copy)" stays English, as a file's name does
    not change with the window's language.
- **What stays English:**
  - identifiers, settings' keys and values, the query language;
  - the logs, and what the helpers tell Skwir;
  - the keymap's notation (the keys it shows are said: Ctrl, Strg);
  - a file's name.
