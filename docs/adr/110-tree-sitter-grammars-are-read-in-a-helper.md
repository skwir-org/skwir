# ADR-110: Tree-sitter grammars are read in a helper

Status: accepted 2026-10-31 with the user (GitHub #3); built the same
day, as `docs/PLAN.md` ("Grammars and printing") records: checked on
Linux, compiled for Windows and macOS (`just lint-all`), not run there. Extends
ADR-060 (bat's grammars, matched by Ferroni), which stands. Decision 4
revised 2026-11-01: a choice for each language beside Automatic's rule.

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
   version from then on.
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
