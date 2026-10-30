# Indexing and AI

What Skwir indexes beyond names, what models do on top of it, and how
both stay light on the machine. Status: design, 2026-10-26, from a
review with the user; nothing is built. The decisions are ADR-089 to
ADR-095; the binding rules are [`invariants/indexes.md`](invariants/indexes.md)
and [`invariants/models.md`](invariants/models.md); the steps, with
their tests and what each must measure, are PLAN.md's
"Indexing and AI". This note is the ground an implementer starts from:
the contracts, the names, the formats and the reasons. Where it names a
type or a key that does not exist yet, it is naming what to build.

## What exists, and what decides the design

Most of the architecture was decided before any of this, and holds:

- **Names are skwird's** (ADR-050): a separate project, three lifetimes,
  every answer saying how far it can be trusted, a `changes` journal in
  its protocol. Skwir walks without it.
- **An index hints, the walk answers; a proven coverage answers**
  (ADR-026, ADR-050): `skwir_core::hints::HintProvider`, and a listing
  that says what its answer rests on (`location::AsOf`,
  `ListingEvent::AsOf`, the footer's "as of").
- **Fields read on demand** (ADR-023): `skwir_core::fields`, a
  `FieldProvider` per key, answers kept by file identity and version in
  memory and in the deletable cache, never read per row or per frame.
- **Notes live in the desktop's stores** (ADR-027), never in a database
  of ours; an edit is a `set_notes` step with an exact-bytes undo.
- **Provenance** (`entry::Provenance`) already has `Model { name,
  confidence }`: a suggestion until the user confirms it.
- **Plans** (`docs/invariants/operations.md`): every change is reviewed
  unless it is the user's own gesture; `Plan::by` says who proposes;
  `Plan::note` says what was left out and why.
- **The permission layer** (ADR-044, ADR-085): private folders never
  reach an agent or code from outside; an agent's call proposes; what
  it is answered is redacted; a script is the user's own.
- **Helpers and sandboxes** (ADR-041, ADR-045, ADR-031): a helper
  answers `--probe` and `--build`, speaks JSON lines
  (`platform::line_channel`), carries pixels in a scratch file, runs in
  bubblewrap on Linux (`sandbox::plugin`) and ends when idle.
- **Learned indexes** (ADR-026): the tags index is taught by every
  listing and every plan (`Executor::observe`), bounded by `tags.index_mb`,
  in 256 pieces loaded on demand and let go of after minutes unused.
- **Measured ahead:** a trigram index of what `content:` reads
  (`crates/bench/src/trigram.rs`), checked query by query against the
  scan.

The numbers that decide the rest, all from `docs/STATUS-LOG.md` and
skwird's `docs/PLAN.md`:

| What | Measured |
| --- | --- |
| Walk and stat of the reference home, 2.1 M files | 5.6 s warm |
| `content:` scan of the same | 34 to 67 s |
| Trigram index of the same text | 11 % of the text; queries 5 to 175× faster than the scan |
| skwird's check of 652,336 folders | 52 ms warm, 1.9 s cold |
| Tags index, a count of the home folder | 0.4 ms |
| The reference home | 5.8 M entries, 44 % distinct names, mostly development trees |

Three consequences:

1. **Scope matters more than any engine.** A home of 5.8 M entries is
   mostly build trees, package stores and caches. What the user searches
   for is a small part of it, and the desktop's indexers already draw
   that line (GNOME's LocalSearch indexes the XDG user folders; Spotlight
   skips `.noindex`).
2. **Trigrams keep the scan's meaning.** Tantivy (ADR-003) tokenizes and
   ranks; `content:` is a grep, regexes included, and the scan stays its
   correctness reference. Ranking by meaning is a vector's job.
3. **The check is cheap enough to be the fallback.** Without skwird,
   folder change times prove names current in a second cold, so a
   derived index can reconcile at the start of a session instead of
   watching.

On the reference machine (20 threads, 30 GB, an Intel iGPU and an NVIDIA
eGPU): Ollama is installed with one cloud model pulled, whisper-cli,
exiftool, pdftotext, pandoc and ffmpeg are on `PATH`; no tesseract, no
desktop index, no plocate. Nothing below assumes any of them.

## The layers

```
skwird        names, kinds, identity, sizes, times; coverage; a journal      (ADR-050)
   │
   ▼ enumerate, changes since
harvest       core: scope, pace, the journal or the check, coverage words     (ADR-089, 092)
   │
   ▼ one read per file, fields asked ahead
extract       text, hash, perceptual hash, OCR, transcript, facts            (ADR-091)
   │                      │
   ▼                      ▼
fulltext      trigrams    semantic   vectors: static for all, dense learned   (ADR-090, 094)
   │                      │
   ▼ content:             ▼ about:, like:, labels
query plugin  terms, pills, the footer's words, Find Similar
   │
   ▼
models        core: providers by task; the system's own, Ollama, the helper,  (ADR-093)
              a remote by opt-in; what leaves the machine is said and logged
   │
   ▼
suggest · organize · cleaners · agent · query.ask                            (ADR-095)
              pills until confirmed; plans with reasons; steps left out
```

Each layer is an accelerator the one below can replace, and each answer
says what it rests on. The names index can be absent (Skwir walks); the
extract cache can be deleted (facts are read again); an index can be
behind (the footer says as of when, and how much is not read); a model
can be missing (the feature says what it needs). Nothing below ever
blocks the window.

## Core contracts

Only what two plugins must agree on goes in `skwir-core` (principle 5).
Two new modules, and small additions to four existing ones.

### `skwir_core::harvest`

The harvest reads files in scope, once, for every index that wants them.
`extract`, `fulltext` and `semantic` agree on it, so it is the core's.

```rust
/// Where derived indexes reach: roots, and what is left out under them.
pub struct Scope {
    pub roots: Vec<PathBuf>,          // `extract.roots`; default the XDG user folders
    pub excluded_names: Vec<OsString>, // `.git`, `node_modules`, … (`extract.exclude`)
    pub hidden: bool,                 // hidden entries, off by default
    pub max_bytes: u64,               // past it a file is not read (`extract.max_mb`)
}
impl Scope {
    /// Whether `dir`, under a root, is entered: not private (`Privacy`),
    /// not a mount of another device unless the root is on it, not
    /// remote (`reach`), not excluded by name, holding no `CACHEDIR.TAG`,
    /// `.noindex`, `.nomedia` or `.trackerignore`.
    pub fn enters(&self, dir: &Path, privacy: &Privacy) -> Enter;
}

/// How far an index's answer under a root can be trusted (ADR-050's words).
pub enum Proof {
    Live { since: SystemTime },   // every change applied, up to the barrier
    Checked { at: SystemTime },   // reconciled against the folders at `at`
    AsOf { at: SystemTime },      // loaded, not yet reconciled this session
    NotCovered { why: NotCovered }, // Off, Excluded, Remote, Removable, OverRoom, Denied
}
pub struct Coverage { pub root: PathBuf, pub proof: Proof, pub read: u64, pub unread: u64 }

/// One index fed by the harvest. Registered by a plugin, withdrawn with it.
pub trait Indexer: Send + Sync {
    fn id(&self) -> &'static str;
    /// The field keys it needs of a file (`extract.text`, `extract.hash`…).
    fn keys(&self) -> &'static [&'static str];
    /// Cheap, from what a listing knows: kind, type name, size.
    fn wants(&self, entry: &Entry) -> bool;
    /// A batch read; may block; must check `cancel`.
    fn index(&self, batch: &[Harvested], cancel: &CancelToken) -> anyhow::Result<()>;
    fn forget(&self, files: &[FileId]);
    fn coverage(&self, root: &Path) -> Coverage;
    /// Bytes it may still take under its cap (`<id>.mb`); zero stops learning.
    fn room(&self) -> u64;
}
pub struct Harvested { pub entry: Entry, pub path: PathBuf, pub fields: HashMap<&'static str, Value> }

/// Where changes come from: skwird's journal where it covers the root,
/// else the check (folder change times) plus a stat of each file.
pub enum Changes { Journal { token: String }, Check { folders: FolderTable } }

/// When the harvest may run (ADR-092). Read before every batch.
pub struct Pace { /* last input, load, battery, memory pressure, the setting */ }
```

The harvest is one lane at `Priority::Background`, one batch in flight,
on a thread marked idle to the system (`SCHED_IDLE` and the idle I/O
class on Linux; background QoS on macOS; background mode on Windows).
It is started by `extract.build`, by the setting `extract.background`
while the window runs, or by `skwir index` without a window. Its state
(`harvest::Status`: per root, files read, unread, the proof, paused and
why) is what `extract.status` answers and the Index page shows.

**While closed** (ADR-089, points 3 and 4). The switch "Also while Skwir
is closed" on the Index page (`extract.while_closed`) writes an entry
for the user's service manager, in ADR-087's shape: a systemd user
service and timer, a launch agent, a Task Scheduler task, each running
`skwir index` a few minutes after login and then hourly (provisional).
Turned off, the entry is stopped, removed and its manager told; a
manager that fails turns the switch back off with the reason on its row;
turning the plugin off removes it. Without a window the pace's sign of
the user is the session's idle signal (logind's idle hint, the system's
idle time on macOS and Windows); a desktop that gives none leaves the
run reading nothing, said beside the switch. One harvest runs at a time
for a user (a lock in `extract/`); a windowless run ends after its batch
when a window opens, and the window's own setting decides what follows.

What happens to a file, in order: the scope says whether its folder is
entered; a listing (skwird's, or `fs::Dir`) gives the entry; indexers
are asked `wants`; the union of their keys is read through
`Fields::get_all` in one read of the file, which is the extract
provider's; the batch goes to each indexer; the file's identity and
version are recorded as read under the root. A file already read at
this version is skipped at the fields' cache. A file over `max_bytes`,
special, a link, or one whose read fails is recorded as unread with its
reason, counted in `Coverage::unread`, and retried at the next
reconcile, never kept as empty (skwird's rule, ADR-050).

### `skwir_core::models`

What a model does, asked by task, answered by whichever provider the
settings put first. `semantic`, `suggest`, `organize`, `agent` and the
query plugin's `query.ask` agree on it.

```rust
pub enum Task { TextEmbedding, ImageEmbedding, Chat, Ocr, Speech, Describe }

/// Where an answer is computed. Said in every call's result, shown in
/// the window, and asked of the user before an agent's call leaves.
pub enum Reach { Local, LeavesMachine { service: String } }

pub trait ModelProvider: Send + Sync {
    fn id(&self) -> &'static str;           // "system", "ollama", "helper", "remote"
    fn title(&self) -> &str;                 // "This Mac", "Ollama", "Skwir's own", the service
    fn reach(&self) -> Reach;
    /// Which tasks it answers now, with which model, its dimension and
    /// languages; cheap, from what it probed (`Plugin::requirements`).
    fn offers(&self) -> Vec<Offer>;
    fn embed_text(&self, model: &ModelId, texts: &[&str], c: &CancelToken) -> Result<Vectors>;
    fn embed_image(&self, model: &ModelId, pixels: &Rgba, c: &CancelToken) -> Result<Vector>;
    fn chat(&self, model: &ModelId, req: ChatRequest, on: &mut dyn FnMut(Delta), c: &CancelToken) -> Result<ChatAnswer>;
    fn ocr(&self, pixels: &Rgba, languages: &[&str], c: &CancelToken) -> Result<Vec<TextLine>>;
    fn transcribe(&self, pcm: &PcmFile, language: Option<&str>, on: &mut dyn FnMut(Segment), c: &CancelToken) -> Result<Transcript>;
    fn describe(&self, model: &ModelId, pixels: &Rgba, prompt: &str, c: &CancelToken) -> Result<String>;
}

pub struct Models { /* providers by owner; the setting per task; the log */ }
impl Models {
    /// The provider the settings put first for `task` that offers it now.
    pub fn for_task(&self, task: Task) -> Option<(Arc<dyn ModelProvider>, ModelId)>;
    /// Every call goes through here: it refuses a private path, records
    /// what left the machine (`Log`), and, run as an agent
    /// (`permissions::as_agent`), asks the user first when `reach`
    /// leaves (`App::ask`, "An agent asks to send … to …").
    pub fn call<T>(&self, task: Task, what: Sent, f: impl FnOnce(&dyn ModelProvider, &ModelId) -> Result<T>) -> Result<T>;
}
```

A `ChatRequest` carries messages, optional tools (the command registry's
declarations, as the MCP server already renders them), an optional JSON
schema the answer must match (constrained decoding where the backend
has it, validated after otherwise), and `max_tokens`. Every string that
came from a file name or a file's content is marked untrusted
(`Message::untrusted(text)`), so a provider's prompt wraps it as data
and the agent plugin never lets it name a tool.

### Additions to existing modules

- `hints::HintProvider` gains `coverage(root, key) -> Proof` (ADR-050's
  step A) and `scored(key, value, root, limit, cancel) ->
  Option<Vec<(PathBuf, f32)>>` for terms that rank (`about:`, `like:`).
  A search with a scored term orders its results by `keys::SCORE`, which
  the query plugin sets on each entry it publishes.
- `entry::keys`: `SCORE` ("score", a float, never written to a store),
  `HASH` ("hash", BLAKE3 of the content as hex, provenance `Plugin`).
- `App::last_input()`: an `AtomicU64` of the last input's instant,
  stamped by the shell on key, pointer and wheel events (one store per
  event; nothing else). The harvest's `Pace` reads it.
- `op`: a step may carry a mark (`Operation::mark() -> Option<StepMark {
  confidence: f32, reason: String }>`). The review shows the reason and
  a bar, and lets a marked step be left out; Apply applies the kept
  steps as one plan, one undo entry (ADR-095).
- `settings::page`: `INDEX` ("Index": what is indexed, where, when, how
  much room) and `MODELS` ("Models": which answers each task, what
  leaves the machine, model packs).
- `fields::Fields::teach(key, identity, value)`: a value learned
  elsewhere (a preview's text) kept as the registered provider's answer
  at its current version; dropped when no provider answers `key`.
- `location::AsOf` carries `unread: Option<u64>` so the footer can say
  "by meaning, as of 14:02; 1,204 files not yet read".

## Plugins

One crate per plugin under `crates/plugins/`, depending on the core and
the platform only. Each is off until the user turns it on, except
`extract`, `fulltext` and `cleaners`, which cost nothing idle and read
nothing unasked.

### `extract`

The `FieldProvider` of derived facts, and the owner of the harvest's
settings and status.

| Key | Value | Read from |
| --- | --- | --- |
| `extract.text` | text, bounded (`extract.max_mb`), with the pages' or parts' offsets | a text file's bytes as `content:` reads them; a document's text as its preview reads it (PDF through the reader helper, Office and RTF through the documents plugin's readers or Pandoc, EPUB, email, Markdown); never a special file |
| `extract.hash` | BLAKE3 hex | the same read, when the file was read whole |
| `extract.phash` | 64-bit perceptual hash | the thumbnail (`thumbs`), never the original |
| `extract.ocr` | text lines with boxes | the thumbnail at the largest side the service keeps, through `Models` (`Task::Ocr`); off unless `extract.read_pictures` is on |
| `extract.transcript` | timed segments | sound decoded to PCM by the media helper into a scratch file, through `Models` (`Task::Speech`); off unless `extract.transcribe` is on |
| `extract.facts` | a map | EXIF, media facts the platform already reads, page counts |

Rules: one read per file per version; the reader is the preview's
(`preview::is_office`, `shown_as_pages`, the same bounds), so what the
index holds is what the preview shows; a read failure is retryable,
never an empty answer; a private file is never read here (`Privacy`).

Commands (`register(..)` effects in brackets): `extract.status` [data],
`extract.build {root}` [acts], `extract.pause` and `extract.resume`
[acts], `extract.forget {root}` [acts], `extract.text {entries, limit}`
[reads] (answers the text kept, for agents, bounded and redacted as any
answer).

Settings (page Index; titles sentence case, descriptions sentences):

| Key | Title | Default |
| --- | --- | --- |
| `extract.roots` | Folders indexed | the XDG user folders |
| `extract.exclude` | Folders left out | `.git`, `.hg`, `.svn`, `node_modules`, `__pycache__`, `.venv`, `venv`, `.cache`; and always `CACHEDIR.TAG`, `.noindex`, `.nomedia`, `.trackerignore`, private folders |
| `extract.background` | Index in the background | Off; While idle; Always while open |
| `extract.while_closed` | Also while Skwir is closed | off; essential; writes or removes the service manager's entry (ADR-089) |
| `extract.max_mb` | Largest file read | 32 |
| `extract.mb` | Room for what is read | 512 (provisional) |
| `extract.removable` | Removable drives | Ask once; Never |
| `extract.read_pictures` | Read text in pictures | off |
| `extract.transcribe` | Transcribe sound | off |

Cache: the `fields` namespace holds the answers, as for any provider; the
`extract` namespace holds `by-hash/` (content hash → the version whose
facts are kept, so a copy or a move reuses them) and `harvest/<root>/`
(the folder table of the check, the files read, the journal token).

**Learned from looking.** The previews that extract text (the reader's
text layer, the documents plugin, EPUB, email, Markdown) hand it to
`Fields::teach("extract.text", identity, text)` once they have it, on
their worker. The index then grows from what the user looks at with
background indexing off. The thumbnail service likewise offers the
perceptual hash of what it decoded (`extract.phash`).

### `fulltext`

A trigram index of `extract.text`, the one that was measured. It answers
`content:` through the hints with a proof, and the query plugin skips
the scan under a covered root.

- **Format,** per root (`fulltext/<root>/`): segments of about 32 MB of
  text as the bench builds them; each a document table (file, version,
  text length, offsets of its parts), its trigrams sorted, each with
  its posting list (delta-encoded varints), a tombstone list; a header
  with the layout version and every array's width. Segments are
  added; never is the index rewritten whole. Compaction merges segments
  in the background when tombstones pass a quarter of a segment or
  segments pass sixteen. A damaged or older file is rebuilt.
- **Queries** are planned from the regex's HIR as the bench does
  (`trigram.rs`), letters folded, classes expanded up to a bound, so the
  index names every file that can match and the searcher
  (`lang::content_matches`) decides on the text, a document's as the
  extract provider keeps it (`Fields::get`, extracted again if it was
  let go of). A pattern that leaves no trigram (two letters, a class too
  wide) walks.
- **`content:` on extracted text.** The term matches what a file reads
  as text: its bytes for a text file, as today; its extracted text for a
  document. The scan, the correctness reference, extracts on the fly
  under the same bounds where the index does not cover, so a search is
  slow there and never wrong. (Decided over a second term `text:`,
  ADR-090.)
- Settings: `fulltext.mb` (Room for the text index, 256 provisional).
  Command: `fulltext.status` [data].

### `semantic`

Vectors, and the terms and chips that use them.

- **Two tiers.** A static embedding model (model2vec's layout: a token
  table and an embedding matrix, mean-pooled; tokenized by the
  `tokenizers` crate, the matrix memory-mapped) embeds every name, path
  and text chunk in scope, in process, microseconds each. A dense model
  (through `Models`, `Task::TextEmbedding`) embeds the query, reranks
  the top candidates, and its vectors are kept for what was reranked or
  looked at, so the dense tier is learned where it is used.
- **Chunks.** A name and its parent folder's names are one chunk; text
  is cut at about 256 tokens on sentence or paragraph boundaries with an
  overlap, each chunk keeping its byte range in `extract.text` for the
  snippet the preview shows.
- **Pictures.** An image-text model's image tower (MobileCLIP or CLIP,
  ONNX, in the helper) embeds the thumbnail; its text tower embeds a
  query, or a list of labels for zero-shot labels (`semantic.labels`, a
  field: labels with confidences, provenance `Model`).
- **Format,** per root and tier (`semantic/<root>/<model-id>/`):
  segments of a file table (file, version), a chunk table (file, ordinal,
  byte range), int8 vectors with a scale each, tombstones; the model's
  id, version and dimension in the header, so another model's vectors
  never pass for these. Search is a brute-force dot product over the
  memory-mapped int8 arrays with SIMD, a top-k heap; past a million
  vectors in one root an approximate index (usearch's HNSW) is decided
  then, by measurement.
- **Terms,** owned by the query plugin and answered through
  `hints::scored`: `about:"text"` (meaning, text and names, pictures by
  the image-text model) and `like:"path"` (the file's own vectors:
  meaning for text, looks for a picture; the kind decides). Find Similar
  gains `By::Meaning` and `By::Looks`. A search with either term orders
  by `keys::SCORE`, and its footer says "by meaning, as of …, N files
  not yet read". Under a root not covered it lists what the index has,
  says what it leaves out, and links to `extract.build`.
- Settings: `semantic.text` (Vectors of text: Names only; Names and
  text), `semantic.dense` (Rerank with the dense model: on when a
  provider offers it), `semantic.images` (Vectors of pictures), and
  `semantic.mb`. Commands: `semantic.status` [data], `semantic.search
  {text, root, limit}` [reads], `semantic.similar {entry, root, limit}`
  [reads].

### `models`

The providers, in the order the settings put them, each probed as a
requirement:

| Provider | Linux | macOS | Windows |
| --- | --- | --- | --- |
| `system` | none | Vision (OCR, feature prints, classification), NaturalLanguage (`NLContextualEmbedding`), Speech, Foundation Models on macOS 26 (chat with tools) | `Windows.Media.Ocr`; on Copilot+ PCs the Windows AI APIs (Phi Silica, image description, text recognition) |
| `ollama` | its local API, when its socket or port answers; never started by Skwir | the same | the same |
| `helper` | `skwir-infer-worker` (below) | the same, compiled only | the same, under Wine as far as it goes |
| `remote` | an OpenAI-compatible API, or Anthropic's Messages API, by the user's URL and key | the same | the same |
| `installed` | `tesseract`, `whisper-cli` on `PATH`, as programs in the sandbox | the same | the same |

The system's provider is compiled only until a Mac and a Windows machine
exist (ADR-035); it answers `offers()` empty on Linux.

Settings (page Models): `models.text`, `models.chat`, `models.vision`,
`models.ocr`, `models.speech`, `models.describe`: each a choice among
Automatic (the order above, local only), each provider, and Off.
`models.remote.url`, `models.remote.name`, `models.remote.key` (a secret,
kept in the keyring through `platform::secrets`, typed by the user alone,
never an argument), `models.remote.model`. `models.packs`: the model
packs installed, with Fetch and Remove (ADR-093).

Commands: `models.list` [data], `models.log` [data] (what left the
machine: when, to which service, how many bytes, which files by name;
never the content), `models.fetch {pack}` [acts], `models.chat
{prompt, entries?}` [reads when the chat provider is local; an agent's
call that would leave asks first, in `Models::call`].

While any task's provider leaves the machine, the window shows a
persistent indicator in the toolbar, named for the service, with a menu
to the Models page and the log.

### `suggest`

Suggestions of notes, from the vectors and the models, as pills until
confirmed.

- `suggest.tags {entries}` [plans]: a plan of `set_notes` steps, each
  marked with its confidence and reason ("like 12 files tagged
  Invoices"; "the picture shows a beach"), from the file's nearest
  tagged neighbours in the vector index, its zero-shot labels and, when
  a chat provider is on, a small model given the tag vocabulary
  (`tags.list`) and the file's name and snippet. The review is the
  confirmation; applied steps write with `User` provenance.
- `suggest.describe {entries}` [plans]: a comment, from the text's first
  chunk through the chat provider, or a picture's description through
  `Task::Describe`.
- `suggest.peek {entries}` [data]: the suggestions without a plan, for
  Properties' "Suggested" rows and for agents.
- In Properties, suggested tags are pills with a dashed outline and the
  confidence as tooltip; a click adds the tag as the user's own action
  (`tags.edit`); "Confirm all" runs `suggest.tags` on the file. A
  suggested description shows with "Use". `tag:` never matches a
  suggestion; `suggested:` does.
- Settings: `suggest.unasked` (Suggest tags for files shown: off; when
  on, Properties asks `suggest.peek` on its worker for the file shown,
  never for a listing).

### `organize`

Moves proposed by what is already there, ending in a plan.

- `organize.into {root, folders?, entries?}` [plans]: each loose file
  (the root's own files, or `entries`) goes to the nearest prototype
  among the target folders (the root's subfolders by default, or
  `folders`). A folder's prototype is the mean of its files' static
  vectors, and its name's and path's vector when it holds nothing. The
  step is marked with the cosine and the reason ("like 12 files in
  Invoices/2025"); below `organize.threshold` it is marked to be left
  out by default. No language model is involved.
- `organize.propose {root, groups?}` [plans]: the root's files clustered
  on their vectors (k-means over the static tier, k by silhouette up to
  `groups`), each group named without a model by its common name
  tokens, kinds and tags, or with the chat provider by a two-to-four
  word name from its names and snippets; a plan of `mkdir` and `move`
  steps. The groups show first as a Quick filters facet ("Groups"), so the
  user looks before anything moves.
- `organize.tags {root}` [plans]: the same groups as tag suggestions, a
  plan of `set_notes`: an organization that moves nothing (tag places,
  ADR-032; saved searches, ADR-057).
- Settings: `organize.threshold` (How sure before a move is proposed
  checked, 0.6 provisional).

### `cleaners`

Deterministic, no model, each a plan: `cleaners.duplicates {root}`
(by `extract.hash`; the kept copy is the oldest name, the rest trashed;
hard links are not duplicates: `FileId` tells), `cleaners.similar_pictures
{root, distance}` (by `extract.phash`), `cleaners.empty_folders {root}`,
`cleaners.stale_downloads {root, older_than}` (the Downloads folder's
files untouched for the period; `trash`, never `delete`). All [plans].

### `agent`

An in-process loop over the command registry with the chat provider,
run as an agent (`permissions::as_agent`), so its plans are proposed,
its answers redacted and its acting commands asked. `agent.run {goal,
root, entries?}` [plans]. Last in order; the deterministic flows above
cover most of what it would do, and an external agent over MCP already
has every tool.

### The query plugin's additions

- `about:` and `like:` terms; `By::Meaning`, `By::Looks` in Find Similar.
- `query.ask {text, root?, pane?}` [shows]: natural language compiled to
  a query. With a chat provider: the model fills a JSON object matching
  the query grammar (terms, with `about:` for what no term says),
  constrained where the backend supports it and validated by the parser
  always; the prompt holds the grammar, the tag vocabulary and the
  user's words, nothing else. Without one: rules for dates, kinds, sizes
  and extensions, the rest as `about:`. The result opens as pills, which
  the user edits; the address keeps the words typed.
- Under a covered root, `content:` consults `fulltext` through the hints
  and skips the scan; elsewhere it scans, extracting on the fly.

## The helper: `skwir-infer-worker`

One helper for every model the system does not bring, outside
`default-members` like the other native workers, linking ONNX Runtime
(`ort`), llama.cpp (`llama-cpp-2`) and whisper.cpp (`whisper-rs`). Its
cost stays out of Skwir's process: a model is memory-mapped by the
helper, and the helper exits a minute after its last call, as the
grammars are let go of (ADR-060).

- **Protocol:** JSON lines with ids, as the document workers and plugins
  from outside speak (`platform::line_channel`). `initialize {models,
  scratch}` → what it can run; `load {model}`, `unload {model}`;
  `embed {model, texts}` → vectors; `embed_pixels {model, file, width,
  height}` (RGBA8 in a scratch file, as pages travel today); `ocr
  {file, width, height, languages}`; `transcribe {model, pcm, language}`
  with `segment` notifications; `chat {model, messages, tools, schema,
  max_tokens}` with `delta` notifications; `$/cancel {id}`; `log`.
  Experimental until declared stable, as the plugin protocol (ADR-047).
- **Sandbox:** as a plugin from outside (`sandbox::plugin`): the models
  folders read-only, its scratch folder writable, no network, dies with
  Skwir. Nothing of the user's reachable but what a call hands it in
  its scratch folder. On macOS and Windows, ADR-039's isolation.
- **Devices:** CPU by default. A GPU is used only through a mechanism
  that holds on any vendor (llama.cpp's Vulkan backend where a device
  answers; ONNX Runtime's default provider), never by vendor or driver
  name (`AGENTS.md`, "No tuning for the machine").
- **Priority:** started in the background class when called by the
  harvest; the user's own call (a description asked in the preview) runs
  at normal priority.
- `--probe` says which runtimes link and whether any pack is installed;
  `--build` as every helper (`docs/invariants/settings.md`). Listed in
  `tools/package/programs` and `optional`, the packages' file lists,
  `tools/package/{macos,windows}/programs`; notices made (`just
  notices`).

### Model packs

A model is a folder under `skwir/models/<id>/` in the user's or the
system's data folders (as plugins from outside are found), holding
`model.toml` (id, title, task, format: `model2vec`, `onnx`, `gguf` or
`ggml`; dimension; languages; licence; source URL; sha256 and size per
file; the version of its layout) and its files. The repository keeps the
catalogue (`tools/models/catalogue.toml`) the packages are built from
and `models.fetch` reads. A pack never changes what Skwir does, only
what the helper can answer. Candidates, each to be measured before it is
the default:

| Task | Candidate | Size | Why |
| --- | --- | --- | --- |
| static text | `potion-multilingual-128M` or a distillation of our own with a smaller vocabulary | ~130 MB int8 | multilingual (French and English at least), in process, no runtime |
| dense text | EmbeddingGemma 300M q4; `multilingual-e5-small` as the lighter alternative | 200 to 120 MB | multilingual; Matryoshka dimensions allow 256-dim int8 vectors |
| image–text | MobileCLIP-S2; CLIP ViT-B/32 as the fallback | 35 to 90 MB per tower | made for devices; a text tower for queries and labels |
| OCR | PP-OCRv4 detection and recognition | ~15 MB | multilingual; ocrs (pure Rust, Latin only) as an alternative |
| speech | whisper.cpp `base` or `small` quantized | 75 to 190 MB | multilingual; whisper-cli where installed |
| chat | Qwen3 4B instruct q4 (16 GB machines), Qwen3 1.7B below; Gemma 3n as an alternative | 1 to 2.5 GB | tool calling, 100+ languages, Apache-2.0 |
| describe | SmolVLM 500M; Moondream 2 | ~0.5 to 2 GB | on demand only |

Any download is the user's own act (`models.fetch`, the Models page's
Fetch), shows progress, verifies the hash, and is the one new use of the
network (ADR-093, amending ADR-020's rule in `AGENTS.md` once built).
Packages may carry packs instead (`skwir-models-text`, `-vision`…).
Ollama manages its own models; the system's bring theirs.

## Freshness

- **With skwird** covering a root live, the harvest asks `changes {root,
  since}` from its token at each run and re-reads only what changed;
  `Proof::Live` with skwird's barrier. "Too old" or a coverage drop
  falls back to the check.
- **Without,** at each run (a session's start while idle, `extract.build`,
  `skwir index`): the folders whose change time moved since the folder
  table are listed again (names proved, skwird's check in Skwir's
  harvest: a folder's change time read before its entries, a second
  change within a tick read again next time, device and inode
  compared), then every file in scope is stat-ed and its version
  compared with what the fields keep: a moved version is re-read. The
  result is `Proof::Checked { at }`. On a filesystem whose change times
  do not tell (`filesystem::change_times_tell`), every folder is listed.
- **Between runs,** `Executor::observe` tells the indexers what Skwir's
  own plans renamed, moved, trashed or wrote, as it tells the tags
  index: a file edited in Skwir is current at once.
- **A loaded index** is `Proof::AsOf { at }` until the first reconcile of
  the session finishes; a search then says so.

## Pacing

The harvest and the helper run only when the machine has nothing better
to do (ADR-092):

- no input to Skwir within the last two seconds (`App::last_input`), or
  the desktop's idle hint where it has one;
- the one-minute load average below the core count (Linux and macOS);
  the system's own busy signal on Windows;
- not on battery (UPower on Linux; `IOPSCopyPowerSourcesInfo` on macOS;
  `GetSystemPowerStatus` on Windows), unless `extract.background` is
  Always;
- no memory pressure (PSI `some avg10` under ten percent on Linux; the
  memory pressure notifications on macOS and Windows);
- a batch of at most 64 files or 32 MB between checks, so the harvest
  pauses within one batch of the user's return.

What it does to stay unnoticed: the idle scheduling class and I/O
priority; `posix_fadvise(DONTNEED)` on a file over a megabyte once
extracted, so the user's working set is not evicted; one file at a time
on a rotational drive (the kernel's `rotational` flag; the USB drive
lesson); never a remote or an unasked removable drive; one inference at
a time in the helper; models memory-mapped so their pages go back when
the helper exits.

Budgets, in `crates/bench/budgets.toml` once measured, kept by
`just bench-check`: `extract_text_us` (a thousand text files),
`trigram_segment_us` (one 32 MB segment built), `trigram_query_us`,
`static_embed_us` (a thousand names), `vector_search_us` (100,000 int8
vectors), `harvest_yield_ms` (from the user's input to the harvest
pausing; at most one batch), and `just idle-pss` with every AI plugin
on and idle equal to the baseline.

## Privacy and what leaves the machine

- Private folders (`permissions::Privacy`, `.private`) are never read by
  the harvest, never indexed, never handed to the helper, never named in
  a model's prompt: `Scope::enters` asks `Privacy` before a folder, and
  `Models::call` refuses a path it is handed that is private.
- A model's answer is data: a suggestion with `Model` provenance, a
  plan's step, a query's pills. No answer ever runs a command: the agent
  plugin maps a tool call to `CommandRegistry::dispatch` as an agent, so
  it proposes and is asked, as an external agent is.
- File names and contents are untrusted in every prompt: wrapped as
  data, never as instructions; a prompt's instructions come from the
  plugin's own source.
- Local is the default of every task. A remote provider is chosen in
  Settings, per task, and shows the indicator while chosen; what leaves
  is logged by file and byte count; an agent's call that would leave is
  asked first.
- Nothing is fetched because a file names it. The one download is a
  model pack the user asked for, by the catalogue's URL, hash checked.

## The user's view

- **Index page:** the roots and what is left out; the background setting;
  the switch for while closed, with its entry's last run and, when it
  read nothing, why;
  per root, files read and not read, the proof in words ("live with
  skwird", "checked at 14:02", "not indexed: on a network"), room used;
  Pause, Index now, Forget.
- **Models page:** per task, who answers and where; the indicator's
  words; the packs with Fetch; the remote's URL, name and key.
- **The footer** of a search answered by an index: "as of 14:02", "by
  meaning", "1,204 files not yet read", with the tooltip saying what
  answered and what that leaves out, as plocate's answer says today.
- **Properties:** "Suggested tags" and "Suggested description" rows,
  dashed pills, a tooltip with the confidence and the reason; "In the
  index: read 14:02, 1,230 words, 12 pages".
- **Find Similar:** "by meaning" and "by looks" chips; a result's
  tooltip and the preview's header show the matching snippet.
- **Quick filters:** "Groups" when asked, from `organize.propose`.
- **The review:** marked steps show a reason and a confidence bar and a
  checkbox; "Leave out the unsure" unchecks those under the threshold;
  Apply applies the kept.
- **The toolbar indicator** while a provider leaves the machine.

## Measuring first

As the trigram was measured before it was decided, each tier is measured
before its default is set. New `skwir-bench` commands, on synthetic data
that keeps the properties that matter and names nothing real:

- `skwir-bench embed`: for each candidate static and dense model, names
  and chunks per second on CPU, the index's bytes per chunk, and
  retrieval quality on a bilingual synthetic set (French and English
  paraphrases of file names and short texts: recall at 10 of the
  paraphrase, with and without the dense rerank). Decides the static
  model, the dense model, and whether the static tier alone is enough
  for names.
- `skwir-bench harvest`: files per second through `Fields::get_all` for
  text files and PDFs made by the bench (`printpdf` or hand-written
  streams), the pause latency after a simulated input, idle CPU with
  the harvest paused, PSS of the helper with each model loaded and after
  it exits.
- `skwir-bench trigram` already exists; it gains the extracted text of
  documents and the segment format.
- `skwir-bench images`: thumbnails embedded per second, zero-shot label
  precision on a labelled synthetic set (shapes, colours, rendered words).

Each report says counts, sizes and times, never a name.

## Order of work

PLAN.md's "Indexing and AI" lists the steps, A to H, with what each is
done when. In short: the extract provider and `content:` over documents
first, with no model; then the trigram index and its coverage; then the
cleaners; then vectors over names, measured first; then pictures; then
the providers and the helper; then suggestions, organize and the
natural-language address; the agent last. Each step is useful alone and
leaves the window as light as before when its plugin is off.

## Open questions

1. **Background indexing's default.** Off in the design, so an install
   costs nothing until asked; the desktops default to on for the user's
   folders. Revisit once the harvest's cost is measured.
2. **The static model.** Whether a multilingual model2vec model is good
   enough for names, or a distillation with a smaller vocabulary is
   needed; the bench answers.
3. **The dense tier on CPU.** Whether reranking a hundred chunks stays
   under a second on the reference machine, or the rerank is kept for
   what was already embedded.
4. **`content:` on extracted text,** decided (ADR-090) over a separate
   `text:` term; watch whether a scan over a folder of documents, now
   extracting on the fly, surprises.
5. **One helper or several.** ONNX Runtime, llama.cpp and whisper.cpp
   in one helper is simplest to ship; if its binary or its start time
   is too large, split by runtime behind the same protocol.
6. **Apple's and Windows' providers** are compiled only until the
   machines exist (`docs/HELP-WANTED.md`).
7. **The query grammar as a schema** for `query.ask`: whether the
   parser's `Expr` can be rendered as a JSON schema that constrained
   decoding follows, or the model writes the query language's text and
   the parser rejects what it cannot read.

## What skwird's design must take in

skwird is its own project and this note does not edit it; its session
should record:

- **Open question 4 is answered: content stays out of skwird.** Derived
  facts and their indexes are per user, built by the user's Skwir, never
  by a service that reads other users' files (ADR-089).
- **The journal early.** `changes {root, since}` is what Skwir's harvest
  consumes; it should come with phase 2 (the user service) rather than
  later, carry the entry's identity and kind, and say "too old" rather
  than guess.
- **A query by kinds, size and time under a root** is how the harvest
  enumerates without a walk; it is in the protocol sketch already.
- **Coverage words are shared:** live since, checked at, as of, not
  covered and why. Skwir's `harvest::Proof` uses the same four.
