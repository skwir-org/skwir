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
     plural category or on an argument's word (a kind), attributes. No
     terms, no message referring to another, no functions: arguments
     arrive formatted (5).
   - **Isolation marks are off**: a name isolates itself (5).
   - **A check holds every catalog to its English**: the same ids, the
     same arguments and attributes, every selector with its default, and
     Fluent's syntax.

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
   - **If the idle reading or the first frame shows the cost**, the next
     step is translations compiled into indexed tables at build time, as
     Slint does.

5. **Arguments are typed, and formatted before the message reads them.**
   - **A name** (`Name`: a file's, a place's, a tag's) is quoted as its
     language quotes ("…", « … », „…“) and isolated from the sentence
     around it where a window draws it.
   - **A name's bidirectional controls are shown, never obeyed** (the
     user's choice). The overrides, embeddings and isolates (U+202A to
     U+202E, U+2066 to U+2069) are each drawn as a small muted tag
     naming it (RLO), wherever a name is drawn:
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
     make its words agree.
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
     `LC_NUMERIC`, apart from messages.
   - **On macOS and Windows:** the preferred languages and the region's
     formats on macOS; the display languages and the user's locale on
     Windows.
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

13. **Shipping.**
    - **Catalogs are compiled into the program**; a plugin from outside
      brings its own.
    - **The desktop file and the AppStream metainfo** are translated
      through gettext's PO, merged when packaged (`msgfmt --desktop`,
      `--xml`).
    - **macOS's bundle declares the languages shipped**
      (`CFBundleLocalizations`, one `.lproj` each), so its own menus and
      the per-app language follow. The Windows installer stays English.
    - **When a language is offered** (the user's choice). Automatic
      offers a language once 90 % of its messages are translated and
      reviewed, and every message of what is on by default (core, the
      window, the plugins on by default) is. Below that, Settings lists
      it as incomplete with its share, for whoever chooses it, and
      Automatic never picks it. An offered language that falls under
      75 % for a release goes back to incomplete.
    - **A machine's or a model's translation is a suggestion** until a
      person who speaks the language has accepted it (ADR-000). A draft
      is kept marked as one, and counts as reviewed only once accepted.
    - **French first, drafted by a model and reviewed by the user** (the
      user's choice). The draft follows a glossary settled with them
      first: Skwir's own terms (entry, place, pane, trail, shelf, lens,
      plan, Activity) and French typography (« » and U+202F before
      `; : ! ?`). The review is done in the window running in French,
      surface by surface, with a list of the messages that have plurals
      and kinds.
    - **Translators work on Hosted Weblate** (its Libre plan; Skwir is
      Apache-2.0 and public), opened once every crate is converted
      (PLAN.md's step E). Before that, ids still move, and on Weblate a
      renamed id loses its translations.

14. **Plugins from outside get protocol 2**, a track of its own.
    `InitializeParams` carries the languages; the manifest's titles
    become ids into the plugin's own `l10n/`; an answer may carry
    `{ id, args }`, which the host says from that catalog. A plugin on
    protocol 1 is shown as it writes.

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
