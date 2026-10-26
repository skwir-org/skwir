# ADR-095: Suggestions and organizing end in reviewed plans: Model provenance until confirmed, steps with reasons that can be left out, organizing by prototypes

Status: accepted, 2026-10-26, the user's choice
(`docs/indexing-and-ai.md`). Not built. Applies
ADR-000's principle 6 (AI output is a suggestion until a human confirms)
and extends the review (`docs/invariants/operations.md`).

## Context

- `entry::Provenance::Model { name, confidence }` exists; nothing sets
  it. Notes are written to the desktop's stores (ADR-027), never to a
  database of ours. A plan is reviewed whole: Apply or Cancel.
- Phase 7's flows: organize this folder, rename these consistently,
  what is safe to delete, tag the selection, summarize in the preview,
  always ending in a plan.
- Organizing does not have to move anything: a tag is a place (ADR-032)
  and a saved search is kept (ADR-057).
- A folder's files already say what belongs in it; a nearest-centroid
  classifier over their vectors (ADR-094) places a new file in
  milliseconds and can say why. A language model is needed only to
  name a group or to read an instruction.
- Natural language is the easiest way to ask for a search, and the
  query language is the one contract scripts, agents and the window
  share.

## Decision

1. **A suggestion is a field with `Model` provenance** (`suggest.tags`,
   `suggest.description`, `semantic.labels`), read on demand (never for
   a listing), shown in Properties as pills with a dashed outline and
   the confidence and reason in the tooltip. Confirming one is the
   user's own edit (`tags.edit`, a comment set): written with `User`
   provenance through the stores. `tag:` never matches a suggestion;
   `suggested:` does.
2. **A suggestion for many files is a plan** of `set_notes` steps
   (`suggest.tags {entries}`, `suggest.describe {entries}`); the review
   is the confirmation.
3. **A step can carry a mark** (`Operation::mark`: a confidence and a
   reason, "like 12 files in Invoices/2025"). The review shows the
   reason and a bar, and a checkbox per marked step; "Leave out the
   unsure" unchecks those under the plan's threshold; Apply applies the
   kept steps as one plan with one undo entry. A plan without marks
   reviews as today.
4. **Organize into an existing tree by prototypes, with no model:**
   `organize.into {root, folders?, entries?}` places each loose file at
   the nearest prototype among the target folders (the mean of a
   folder's files' static vectors; its name's and path's vector when it
   holds nothing), each step marked with the cosine and the reason,
   left out by default under `organize.threshold`. **Free-form by
   groups:** `organize.propose {root, groups?}` clusters the root's
   vectors, names each group without a model (common name tokens,
   kinds, tags) or with the chat provider (two to four words from its
   names and snippets), shows the groups first as a Narrow by facet,
   and proposes `mkdir` and `move` steps. **Without moving:**
   `organize.tags {root}` offers the same groups as tag suggestions.
5. **Natural language in the address:** `query.ask {text, root?, pane?}`
   compiles words to a query: with a chat provider, the model fills the
   query grammar (constrained where the backend has it, always
   validated by the parser) from the grammar, the tag vocabulary and
   the user's words, nothing else; without one, rules for dates, kinds,
   sizes and extensions, the rest as `about:`. The result opens as
   pills, which the user edits; the words typed stay in the address.
6. **The agent plugin** is a loop over the command registry with the
   chat provider, run as an agent (`permissions::as_agent`): its plans
   are proposed, its answers redacted, its acting commands asked; it
   never applies. It comes last; the flows above cover most of what it
   would do, and an external agent over MCP already has every tool.
7. **Cleaners are deterministic plans:** duplicates by `extract.hash`
   (hard links told apart by `FileId`), similar pictures by
   `extract.phash`, empty folders, stale downloads (trashed, never
   deleted). No model.

## Consequences

- The core's `op` gains `StepMark` and a plan applied as its kept steps
  (one plan id, one history entry); the review draws marks; the
  operations invariants record that a marked step left out is not a
  failure.
- `organize.threshold` and `suggest.unasked` settings; Properties'
  "Suggested" rows; the Groups facet in Narrow by; the `suggested:`
  term.
- Tests: a plan of suggestions applied then undone restores each
  store's exact bytes; a step left out is not applied and not in the
  undo; `organize.into` on a synthetic tree places by the folder's
  files, says why, and marks the unsure; `query.ask` without a provider
  yields pills the parser reads back; an agent's `models.chat` through
  a remote provider is asked first; nothing private reaches a prompt.
- Each flow's need of the models by task is listed in
  `docs/indexing-and-ai.md`; a flow degrades to its deterministic form
  when a task has no provider, and says so.
