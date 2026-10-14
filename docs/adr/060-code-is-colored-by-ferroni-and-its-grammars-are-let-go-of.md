# ADR-060: Code is colored by Ferroni, and its grammars are let go of

Status: accepted and built, 2026-10-14.

## Context

- **A window left open overnight held 688 MB** (btop's RSS; 667 MB PSS).
  Its heap held 439 MiB of live allocations out of 473 MiB resident,
  read from mimalloc's page table: not the allocator keeping freed
  memory. About 4,100 compiled regular expressions, with their caches,
  were the largest part. Thumbnails (within their 80 MiB), gpui's frame
  buffers at their high-water mark and the playing track's covers made
  most of the rest.
- **Coloring code kept every grammar compiled, forever.** syntect
  compiles a grammar's patterns when a file first needs them, and
  `code_preview::syntaxes` kept the set in a static. Its engine,
  fancy-regex, hands most patterns to regex-automata, which builds
  automata per pattern and a lazy DFA cache per pattern and per thread
  that runs it. In a window on Xvfb, 22 small code files took the heap
  from 22 to 280 MiB, then 327 after five passes (the workers each grew
  their own caches); a 240-byte TypeScript file alone added 106 MiB.
  Leaving the folder gave nothing back.
- **The engines, compared** on bat's grammars (two-face), 59 public
  files in about 35 grammars, single process per engine, machine load
  5 to 14 (2026-10-14):

  | | fancy-regex | Oniguruma (C) | Ferroni |
  | --- | --- | --- | --- |
  | Corpus, one thread | +589 MiB | +43 MiB | +67 MiB |
  | Corpus, 8 threads | +1,352 MiB | +53 MiB | +77 MiB |
  | Every pattern compiled | 1,207 MiB | 75 MiB | 147 MiB |
  | First pass (compiles) | 4.9 s | 0.9 s | 1.7 s |
  | Warm pass | 2.3 s | 0.72 s | 0.56 s |
  | One 1.2 MB line of TypeScript | 23 s | 97 s | 9 s |
  | Patterns refused | 0 of 21,739 | 0 of 22,257 | 2 of 22,257 |

  The three engines gave the same scopes on all 27,293 lines, given the
  same grammars. Ferroni was measured with Ruby's syntax at first, which
  refused VimHelp's look-behind `(?<=^\s*)` as Oniguruma does under it;
  with Oniguruma's own, the onig backend's, its scopes equal Oniguruma's
  byte for byte at the same cost. Deep nesting (20,000 brackets of JSON)
  and long lines ended in each, in 8 MiB stacks like the workers'.
  Ferroni refuses two patterns of Vue Component that Oniguruma 6.9.10
  takes: a class with byte escapes past `\x7f` (`\x7f-\x9f`), which
  Oniguruma's last source refuses in UTF-8 too (April 2025, after 6.9.10,
  the source Ferroni ports: ferroni#227).
- **Oniguruma ended on 2025-04-24**; its repository is archived and
  6.9.10 is the last release. It is C, with a history of memory-safety
  advisories, and Skwir colors the text of any file selected.
- **Ferroni** is a line-by-line port of Oniguruma to Rust (BSD-2-Clause,
  0.4% unsafe), first released 2026-02, 2.1.3 on 2026-10-13: one owner,
  feature-complete by its own account, maintained for fixes. syntect has
  no engine on it, and two-face selects its grammar set by engine.

## Decision

1. **Grammars match with Ferroni.** syntect and two-face are vendored
   with a patch each (`vendor/syntect/README.skwir.md`,
   `vendor/two-face/README.skwir.md`): syntect gains a `regex-ferroni`
   engine with the onig backend's syntax (Oniguruma's own) and options,
   and a pattern it
   refuses never matches instead of panicking its grammar; two-face's
   `syntect-ferroni` selects Oniguruma's grammar set, all of bat's.
2. **Compiled grammars are shared, then let go of.**
   `code_preview::syntaxes` returns the set as an `Arc`: a highlighter
   holds it while it colors, a resume point (ADR-051) while it may be
   resumed, since its parser state indexes into that set. It is kept
   `GRAMMARS_KEPT` (60 s) after its last use, then let go of; the
   workers that compiled it are asked to give back what was freed
   (`scheduler::release_on_workers`), since mimalloc returns what
   another thread frees only when the thread that allocated it collects.
   Reading the set again takes 2 ms; a file's first coloring, its
   grammar compiled again, took 20 to 90 ms in the corpus above.
3. **Coloring stays on any worker.** One lane for it was considered for
   fancy-regex's per-thread caches; Ferroni's per-thread state is about
   0.2 MiB for eight threads.
4. **Oniguruma is the way back.** Should Ferroni be abandoned or break,
   syntect's and two-face's own `onig` features restore Oniguruma, with
   the same grammars and the same scopes, and a C library to build.

## Consequences

- In the window on Xvfb (release build, 2026-10-14), the 22 files add
  31.5 MiB on their first pass (221 before) and 34 after five (305),
  TypeScript 8 MiB (106). A minute after leaving the folder the heap is
  back to 35 MiB from 56, against 16 in an empty folder at start; the
  same files as plain text leave 27, so code leaves about 7 MiB of its
  own, not its grammars. Over six rounds of browsing the files and
  leaving them for a minute, what stays grows by less each time (34.6,
  36.3, 37.4, 38.5, 39.0, 39.3 MiB) and the heap's height while browsing
  stays at 58 MiB from the fourth.
- Six grammars arrive with Oniguruma's set: ARM Assembly, JavaScript
  (Babel), LiveScript, PowerShell, Sass and Salt State SLS. Vue
  Component loses the two patterns Ferroni refuses (an attribute's
  name followed by something unexpected). The grammar means the code
  points U+007F to U+009F, as fancy-regex reads it; said `\x{7f}-\x{9f}`
  it compiles in every engine (vuejs/vue-syntax-highlight).
- The code view's first file after a minute without code waits for its
  grammar to compile again.
- The release binary's code grows by 0.77 MB (Ferroni in, fancy-regex
  out; regex-automata stays for the `regex` crate).
- Two vendored crates and their patches are kept up to date, offered
  upstream, and dropped when released.
- Ferroni's limits (retry, time, stack) are left at their defaults, as
  syntect leaves Oniguruma's; a single line is still not cancellable
  mid-line.
