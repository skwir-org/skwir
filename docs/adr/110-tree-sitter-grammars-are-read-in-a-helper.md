# ADR-110: Tree-sitter grammars are read in a helper

Status: accepted 2026-10-31 with the user (GitHub #3); built the same
day, as `docs/PLAN.md` ("Grammars and printing") records: checked on
Linux, compiled for Windows and macOS (`just lint-all`), not run there. Extends
ADR-060 (bat's grammars, matched by Ferroni), which stands. Decision 4
revised 2026-11-01: a choice for each language beside Automatic's rule.
Decision 3 amended 2026-11-04 with the user, from a page of samples: a
grammar found whole or combined by one rule, said and checked (below).

## Context

- **Code is colored by bat's grammars** (`.sublime-syntax`, ADR-060): a
  language bat does not know is plain text. GitHub #3 asks for any
  language a tree-sitter grammar exists for, the user adding grammars of
  their own.
- **A tree-sitter grammar is code.** It is a parser compiled from C (a
  shared library exporting `tree_sitter_<name>`) or to WebAssembly, and
  queries (`highlights.scm`, `injections.scm`, `folds.scm`, `tags.scm`)
  that name what to color. Loading a library from a user's folder into
  the window's process would run native code there on every file
  previewed; Wasmtime never enters that process (ADR-046).
- **Grammars are already installed** where editors and the system keep
  them: Neovim's parsers (`parser/<lang>.so`, its `queries/`), Helix's
  runtime (`grammars/<lang>.so`, `queries/<lang>/`), Zed's extensions
  (`grammars/<lang>.wasm`, `languages/<lang>/config.toml` naming file
  types, `highlights.scm`), Emacs's (`libtree-sitter-<lang>.so` in its
  `tree-sitter` folder and the system's library folders, no queries), the
  system's packages (Arch's `/usr/lib/tree_sitter/<lang>.so` and
  `/usr/share/tree-sitter/queries/<lang>/`, Debian's `hx` runtime), and
  the tree-sitter CLI's own (`config.json`'s `parser-directories`, its
  library cache). Only some say which files they color.
- **tree-sitter 0.27 runs WebAssembly grammars on Wasmtime 48**
  (`wasmtime-c-api-impl ^48`). Wasmtime 48 is a long-term release,
  supported 24 months (to August 2028); 50, which the module helper used,
  two. The module helper compiled and passed its and `skwir-extend`'s
  18 tests on 48.0.8 (2026-10-31, a scratch worktree).
- **Text is read where it is shown** (ADR-051, ADR-103) and a preview
  reads its file once (ADR-101): a parser that wants the whole text gets
  the copy the preview holds, never the file again.

## Decision

1. **A helper reads them.** `skwir-tree-sitter-worker` loads native and
   WebAssembly grammars, isolated as the readers are, the grammars'
   folders readable, and parses the text the preview holds whole (up to
   its bound; a longer text is colored by bat's grammar, else plain). It
   answers colors by line, injected languages colored by the other
   grammars found, and folds and an outline when the grammar's queries
   say them. One helper for every view, let go of a minute after its last
   use, as bat's grammars are (ADR-060), whatever is open in it: a text
   asked again is opened again from the copy its view holds. What reads
   a text parsed is answered beside a parse that runs.
2. **One Wasmtime.** The module helper moves to Wasmtime 48, the version
   tree-sitter's WebAssembly grammars need, and follows tree-sitter's
   version from then on. A WebAssembly grammar compiled is kept in
   Skwir's cache (Wasmtime's own cache, configured in code, 256 MiB at
   most, the one folder of the user's the helper writes in): a large
   grammar compiles in seconds, and the helper starts again after each
   unused minute. The cache is trusted no further than the helper: only
   the helper writes it, isolated, and only helpers read it, so a module
   kept there can do no more than the helper could, in a helper again.
3. **Grammars are found where they are kept**: Skwir's own folder
   (`<data>/grammars`, a grammar repository as `tree-sitter build` leaves
   it, its `tree-sitter.json` naming its file types), then Zed's, Helix's,
   Neovim's, Emacs's, the system's and the tree-sitter CLI's. A grammar
   without queries or file types borrows them from another found for the
   same language name, then from bat's grammar of that name, then takes
   its name as an extension; `languages.toml` in Skwir's folder maps the
   rest. A query that does not compile against the library is passed
   over for the next source's (each source's queries are told, tried in
   turn); a grammar none of whose highlights compile colors nothing, and
   bat's grammar colors the text. Each file is named by its real path
   and kept only where it really lies in its source's folders: a
   repository's or an extension's for what a description names (a
   `tree-sitter.json` may say any path, a clone may hold links), an
   editor's or the system's for what is listed there (a link its package
   manager left is followed). The helper reads those folders, and nothing
   else of the user's. Helix's runtimes are read as Helix reads them,
   file by file, the user's first.
4. **A setting chooses the engine** where both know a language
   (`reading.grammars`): bat first (the default) or tree-sitter first.
   Revised 2026-11-01 with the user: that is Automatic's rule, and each
   language may be chosen otherwise on the Reading page (Grammars ›
   Languages, `reading.grammar_for`): bat, the tree-sitter grammar of a
   source that has one (every source's is found, the first usable used
   otherwise), or plain text. One list of languages replaces the lists
   of grammars added and found, which read as settings.
5. **One theme for both.** Tree-sitter's capture names (`@keyword`,
   `@function.method`) are mapped to the scopes bat's themes color
   (`keyword`, `entity.name.function`), a dotted name falling back to its
   longest known prefix; the colors are the palette's, as bat's are.
6. **Grammars added to bat and to Sublime Text** (`.sublime-syntax`,
   bat's `syntaxes` folder, Sublime Text's `Packages` and zipped
   packages) join bat's set in the window's process, as data, not code:
   the set built with them is kept in Skwir's cache, keyed by Skwir's
   build and the files.

## Rejected

- **Native grammars in the window's process** (Helix's and Neovim's
  way): a grammar's crash or a hostile input to its scanner takes the
  window.
- **WebAssembly grammars dropped, or a second Wasmtime**: the user's
  rule was one Wasmtime or no WebAssembly; 48 holds both, longer
  supported.
- **VS Code's TextMate grammars**: syntect reads `.sublime-syntax`
  only; converting them would approximate `while` rules and injection
  grammars. Declined by the user (2026-10-31).
- **Tree-sitter first by default**: Neovim's queries use conditions only
  Neovim knows, which a plain highlighter ignores and over-colors.

## Consequences

- A language with a grammar anywhere on the machine is colored; the
  Reading page lists the grammars found, where, and what is wrong with
  one.
- Plugins from outside run on Wasmtime 48 (an LTS, patched like 50); a
  module compiled by 50 is compiled again once.
- A new helper is packaged, probed and listed as the others are.
- Parsing needs the whole text: past what a preview holds whole, or past
  32 MiB (`syntax::MAX_TEXT`, a Full preview's), a tree-sitter-only
  language is plain; a line past 1 MiB is colored by bat's grammar.

## Amendment (2026-11-04): a grammar found whole or combined by one rule

Accepted 2026-11-04 with the user, from a page of samples and four
questions, each answered as recommended: lending only from the
grammar's own repository, a combined grammar named by its queries'
source first, the folders looked in folded under Languages, checks when
Languages is first shown.

- **Context:** a user's report (2026-11-03). Their Emacs grammars
  offered only Plain text. Once the grammar's repository was in a CLI
  parser directory, the row said "Emacs" (its library's source) though
  its colors and its file types were the repository's. A repository two
  folders below a parser directory was not found, and nothing said so.
  A repository's library went with the CLI's cache when that was
  emptied. Decision 3 combines any source's library with any other's
  queries by language name, unsaid: a row names its library's source
  only; whether the two fit is known only once a file is colored (the
  helper's `Check`, built for the Reading page, is asked by nothing); a
  library in the system's library folders named as Emacs names its own
  is called Emacs's; a language named two ways (`c-sharp`, `c_sharp`)
  is two languages. The 2026-11-03 change made a grammar that nothing
  colors say where to put its repository; the rest stands.
- **Decision** (replacing decision 3's borrowing; finding, confining and
  file types stand):
  1. **Whole or a part.** A grammar is whole when one source holds its
     library and its highlights: a Zed extension, Helix's runtime,
     Neovim's parsers and queries, the system's library and queries, a
     repository built (Skwir's folder, the CLI's). Otherwise it is a
     part: a library alone (Emacs's, a parser whose queries are not
     installed) or queries alone (a repository not built). A whole
     grammar is offered as now; a part is never offered alone.
  2. **One rule combines parts:** a library and the queries of its
     grammar's own repository (Skwir's folder, a CLI parser directory)
     or the system's queries of it. An editor's queries (Helix's, Zed's,
     Neovim's) never color another source's library: they are written
     for that editor's revision of the grammar and its predicates. A
     grammar is known by the symbol its library exports
     (`tree_sitter_<name>`), `-` and `_` alike: Emacs's
     `libtree-sitter-c-sharp.so` and Neovim's `c_sharp.so` are one
     language. File types still come from whoever says them
     (`languages.toml`, the grammar's sources, bat's grammar, its name):
     they name files and run nothing.
  3. **A combined grammar names both parts,** its queries' source first,
     as a whole grammar is named by its source and as the user put the
     repository there: "tree-sitter CLI, with Emacs's library", in the
     picker and in Automatic's label; the repository's own name in its
     detail and in the folders looked in.
  4. **A combined grammar is checked before it is offered.** When the
     Languages list is first shown (not when the settings open on
     another page, nor when a picker opens: Automatic's label needs the
     answer), the helper loads its library and compiles its queries
     (`Check`), remembered for the run by both files' identity and
     change time. Until answered it is offered as being checked; a
     refusal greys it with the helper's reason. A file colored before
     then checks it the same way. Whole grammars are checked only when
     they color, as now.
  5. **A source is named by its folder.** Emacs's are
     `~/.emacs.d/tree-sitter` and `<config>/emacs/tree-sitter`; a
     library in the system's library folders (`/usr/lib`,
     `/usr/local/lib`, Homebrew's) is the system's.
  6. **Where grammars were looked for is shown** on the Reading page,
     folded under Languages, its header counting what was nearly found:
     each folder read, whether it is there, what it gave (whole
     grammars, libraries alone, repositories not built), and what was
     nearly found with what to do: a repository one folder too deep in
     a parser directory (the CLI reads only the folders directly in
     it), a repository with no library (`tree-sitter build`, or its
     queries color a library found elsewhere), a library with nothing to
     combine (the 2026-11-03 reason), a parser directory that is not
     there.
  7. **A folder kept elsewhere is linked into Skwir's grammars folder:**
     the one explicit way, for a repository kept anywhere or a folder
     Emacs's `treesit-extra-load-path` names, which Skwir never reads.
- **Rejected:**
  - **Discovery dropped, Skwir's folder alone:** predictable, but users
    of Neovim, Helix and Zed lose grammars that work as found, and the
    report's grammars would need a C compiler (their library is Emacs's).
  - **Any parts combined as now, only labelled:** a label says the mix
    but keeps queries written for another editor on a library they may
    not fit, and two sources' queries tried in turn.
  - **A setting listing more folders:** a link in Skwir's folder does it,
    with nothing more to read or to keep.
  - **Asking Emacs for its own rules** (`emacs --batch`): apart, in
    `docs/PLAN.md`'s What is left.
- **Consequences:**
  - What colors today with another editor's queries stops: a Neovim
    parser colored by Zed's or Helix's queries, an Emacs library by
    Helix's. Such a language is offered by the source that holds it
    whole, else bat's grammar, else plain text, its row saying why.
  - A choice kept under a language's old name (`c-sharp`) is read under
    its symbol's (`c_sharp`).
  - Showing Languages starts the helper when a combined grammar is
    found (a library loaded and its queries compiled each); whole
    grammars cost nothing more.
  - `Grammar::lent` holds only the grammar's own repositories' queries;
    the test of every source changes with the rule.
