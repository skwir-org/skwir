# ADR-098: The address field offers what sources answer, a scope before its text

Status: accepted, 2026-10-26, the user's choices for each open point
(below), from the proposal "The scope says where. The text says what."
(a page shared in the session, mock-ups on the README's example folder).
Built on Linux the night of 2026-10-26 (`docs/STATUS-LOG.md`, "The
address field as an omnibar"); Windows and macOS compiled. Amends
ADR-070 (the field's rows) and retires the Jump overlay.

## Context

- The address field (`omnibox.rs`) already went to paths, searched,
  offered five of Jump's places and handed `>` to the palette, but:
  - words typed over a folder filtered it by the whole text inside one
    name, so `dunes jpg` emptied the view though Enter found `dunes.jpg`,
    and `kind:image` emptied it until Enter;
  - Ctrl+F and Ctrl+L differed only by an "In …" pill and whether the
    text started selected, and only Alt+Enter changed where words
    searched;
  - nothing completed the query language, and a search that could not
    parse became a history step showing a listing error;
  - a search's pills turned into plain text once edited;
  - nothing was offered before something was typed;
  - Jump was an overlay of its own and five rows; saved searches, tabs
    and history were out of the field's reach; the field named
    `jump.candidates`, `query.search` and `query.refine`.
- Browsers' address bars hold addresses, searches and history in one
  field: the first row is what Enter does, a completion is drawn after
  the text, suggestions show on focus, a scope chip says where words
  search ("Tab to search"), actions sit among results. Finder turns words
  into tokens; GitHub's search keeps terms as highlighted text.
- The user chose (AskUserQuestion, 2026-10-26): only the scope a solid
  token, terms editable text; Jump's overlay retired, `>` still handing
  over to the palette; recent searches remembered, on by default; the
  live filter narrowing by every term the listing can decide; `@` typed
  first listing scopes; tags and Recent among the scopes; search as you
  type not now.

## Decision

1. **One field, a scope before its text.** The field may hold a scope, a
   token before the text: where words search. Ctrl+F starts with the
   folder shown, Ctrl+L with none and the path selected, Ctrl+J
   (`ui.jump`, same id) with none and nothing typed. Over a search both
   Ctrl+F and Ctrl+L edit it, its root the scope. Backspace at the start
   of the text takes the scope away (Left there selects the token,
   `input::AtStart`); a click on the token, Down on it selected, Alt+Down
   (`ui.omnibox_scopes`) or `@` typed first list scopes: the folder shown,
   the one above, home, the whole disk, the sidebar's folders, tags and
   places. Ctrl+F on a highlighted folder or place makes it the scope.
   Tab stays completion (ADR-070).
2. **Rows come from sources** (`skwir_core::suggest`). A source answers an
   `Ask` (the text, the caret, what it is: nothing, words, a path, a
   place, `@`; the scope; what the pane shows) with rows in sections
   (`Section`: what Enter does, scopes, completions, the reading, terms,
   recent, saved, tabs, history, places, actions, other scopes) and the
   text's spans. The field asks each source again at every change, a job
   each in its lane (`SuggestSource::lane`), the jobs before cancelled;
   an answer is kept by source until the next, the rows about the text
   itself (what Enter does, completions, the reading, other scopes) only
   from an answer to that text. Rows are put by section; an answer
   arriving later never moves the row the arrows chose; Enter pressed
   before the sources answered asks them there and then (they answer from
   memory). Sources are registered by owner (`Registry::suggesters`) and
   withdrawn on disable.
   - The query plugin (`field.rs`): what Enter does with words (the
     search in the scope, `remember` set; Alt+Enter the same in the
     home folder), or why it cannot run; what the term at the caret goes
     on with (`query.complete`: keys, kinds, tags, periods, sizes), its
     rest drawn after the text; the words read as terms (`query.interpret`,
     rules only, applied only when picked); the same search in the folder
     shown, home, the whole disk, and the files containing the words;
     before anything is typed with a scope, the terms as buttons, the
     recent searches and the saved ones; the text's spans
     (`query.tokens`).
   - Jump: the folders and files one goes to whose paths hold every
     word unbroken, those one goes to most before anything is typed.
   - The tags plugin: tags whose names go with the words.
   - The shell (`suggestions.rs`, owner `ui`): the window's other tabs,
     where the pane was (Back), the sidebar's places and saved searches,
     a few commands whose titles hold the words, with their keys.
   - The shell's own path and place completion (ADR-070) stays in the
     field: it reads the folder typed in its lane and keeps its exact
     paths.
3. **What Enter does is the first row**, as before: a source's row runs
   its invocation; one with none says why (an error, a value still to
   type) and Enter waits; one that only writes text (a term button)
   writes it.
4. **The text stays the one truth.** The query plugin's spans draw it
   (`TextInput::set_styles`): keys in the accent, values on `term`,
   operators muted, what cannot be read underlined in the error color.
   Only the scope is a token; a search's pills come back when the field
   lets go. The UI knows no query syntax.
5. **Words typed over a folder filter it by what the listing knows.** The
   core's `filter` asks the filter's reader (`Registry::filter_readers`,
   the query plugin's `narrow::read_filter`): the longest start of the
   text that parses is read; words, `name:` and `ext:` hold for folders
   too, any other term for files, every folder staying, as a narrowing
   keeps them; what only a search can tell (contents, a picture's date or
   place) waits for Enter and the dropdown says so; a filter that reads
   metadata has every file's asked for, and a type is asked once per
   suffix (`lang::Types`), so a key costs 6 to 13 ms over 100 000 names
   in release (400 ms asked name by name). Without the reader, names that
   hold the text, as before. The typed filter's parts are pills after the
   trail, one family with Narrow by's and a tag's (`ReadFilter::parts`,
   each × the text without it).
6. **Recent searches** are those run from the field (`query.search` with
   `remember`, never an agent's), the last twenty, kept in the deletable
   cache under the query plugin's namespace, never logged; Shift+Delete
   (`ui.omnibox_forget`) forgets one (`query.forget_recent`); the setting
   `query.remember_searches` (General › Address, on) turned off forgets
   them all.
7. **For agents and scripts**, `ui.suggest {text, scope?, pane?}` answers
   every source's rows and the spans, changing nothing (`.data()`: it
   answers whoever asks), a path's names read there and then
   (`omnibox::path_suggestions`, amended 2026-10-27).
8. **Jump is the field.** The overlay is gone; `ui.jump` keeps its id and
   keys (Ctrl+J, the vim scheme's `z`).
9. **The field names no plugin.** It runs the rows' invocations and the
   core's commands (`filter`, `navigate`, `open`); it no longer names
   `jump.candidates`, `query.search` or `query.refine`. A scope of a tag
   is searched by the query plugin as the tag every match has, from the
   home folder; any other place (Recent) as what it lists, `in:recent:`
   (`within.rs`): the place opened through its provider, each entry it
   lists tested as the walk tests a file, contents included, read in its
   lane, never below (amended 2026-10-27, from a filtered listing).
10. **`?` alone lists what the field takes** (amended 2026-10-27): the
    shell's own grammar (a path, a place's address from the schemes
    providers open, `@`, words, `>`) and the query plugin's terms with
    examples (`Section::Syntax`); Enter or Tab writes the example.

## Consequences

- Core: `suggest` (Ask, Typed, Section, Suggestion, Span, Answer,
  SuggestSource, Suggester, `merge`, `gather`), `Registry::suggesters`
  and `filter_readers`, `workspace::ReadFilter` and `FilterPart`, the
  pane's filter kept with its reading (`Pane::read_filter`).
- Query: `assist.rs` (`tokens_at`, `problem`, `complete`, `interpret`,
  the commands `query.tokens`, `query.complete`, `query.interpret`),
  `field.rs`, `narrow::read_filter`, `query.forget_recent`, the
  `remember` argument and setting.
- UI: `omnibox.rs` rebuilt on sources, `suggestions.rs`, the token
  (`scope_token`), `TextInput::set_styles`, `send_start` and `AtStart`;
  `jump.rs` removed; `ui.omnibox_scopes`, `ui.omnibox_forget`,
  `ui.suggest`.
- Not built, recorded in PLAN.md: search as you type (an open
  decision), the model half of `query.ask` (Phase G), a plugin from
  outside offering rows (sources are Rust).
- Amended 2026-10-27: `in:` takes a place (`Query::within`), searched by
  `within.rs`; `?` lists what the field takes; `ui.suggest` answers a
  path's names; what the query understood is drawn on a token of its own,
  `Theme::term` (the accent at 18%, as the band is a mark of it), which
  the code background, nearly the page's own in the light look, was not.
  Jump finds each word unbroken in a path read from `~`
  (`rank_by_words`), in the name before its folders: its overlay's
  letters strewn over a whole path offered a row for nearly any search.
