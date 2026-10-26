# ADR-093: Models come from providers, the system's own first; the helper is sandboxed; a model pack is the one download

Status: accepted, 2026-10-26, the user's choice
(`docs/indexing-and-ai.md`). Not built.
Applies ADR-044 (model providers are code from outside: same rules) and
amends ADR-020's rule that only `map-tiles` makes requests, for one act
of the user's: fetching a model pack.

## Context

- Phase 7's plan asked for a `ModelProvider` trait (chat with tools,
  embeddings, image understanding, transcription), a lazy local
  runtime, Ollama when detected, and remote APIs only by explicit opt-in
  with a persistent "leaves your machine" indicator, at zero idle cost.
- Harness the environment (principle 4): macOS brings Vision (OCR,
  image feature prints, classification), NaturalLanguage (sentence
  embeddings), Speech, and on macOS 26 the Foundation Models framework
  (an on-device model with tool calling); Windows brings
  `Windows.Media.Ocr` everywhere and, on Copilot+ PCs, the Windows AI
  APIs (Phi Silica, image description, text recognition). Linux brings
  nothing of the kind. On the reference machine Ollama, whisper-cli,
  exiftool, pdftotext and pandoc are installed.
- Skwir's helpers (PDFium, Poppler, MuPDF, GStreamer, libarchive, the 3D
  model helper) show the shape: a program outside `default-members`,
  JSON lines, `--probe` and `--build`, carried in the packages, isolated
  (ADR-031, ADR-039, ADR-045), ending when idle (ADR-060's grammars).
- The reference machine has an Intel iGPU and an NVIDIA eGPU; the rule
  is no tuning for the machine measured.
- ADR-044: private folders never reach code from outside; an agent's
  call is checked and its answer redacted. ADR-082 and ADR-086: a
  secret is typed by the user alone and kept in the keyring.

## Decision

1. **`skwir_core::models`:** tasks (`TextEmbedding`, `ImageEmbedding`,
   `Chat`, `Ocr`, `Speech`, `Describe`), a `ModelProvider` registered by
   the `models` plugin per backend with what it offers now, a `Reach`
   (`Local` or `LeavesMachine { service }`) said in every answer, and
   one gate every call goes through (`Models::call`): it refuses a
   private path, records what left the machine, and, run as an agent,
   asks the user before a call that leaves ("An agent asks to send … to
   …", `App::ask`).
2. **Providers, in the settings' order per task:** the system's own
   (`system`: Apple's and Windows' frameworks, compiled only until the
   machines exist), installed programs and services (`ollama` when its
   local API answers, never started by Skwir; `tesseract` and
   `whisper-cli` on `PATH`, run in the sandbox), Skwir's helper
   (`helper`), a remote service (`remote`). Automatic means local only.
   Each task's setting (`models.text`, `models.chat`, `models.vision`,
   `models.ocr`, `models.speech`, `models.describe`) can also be Off.
3. **The helper, `skwir-infer-worker`:** ONNX Runtime, llama.cpp and
   whisper.cpp in one program (split by runtime behind the same protocol
   if its size or start says so); JSON lines with ids, pixels and PCM in
   scratch files as pages travel today, streamed `delta` and `segment`
   notifications, `$/cancel`; sandboxed as a plugin from outside (the
   models folders read-only, its scratch folder, no network, dies with
   Skwir); exits a minute after its last call; CPU by default, a GPU only
   through a mechanism that holds on any vendor (llama.cpp's Vulkan
   backend, ONNX Runtime's default provider), never by a driver's name;
   `--probe`, `--build`; listed with the optional helpers and in the
   packages' file lists; notices made.
4. **Model packs:** a folder `skwir/models/<id>/` under the user's or
   the system's data folders, with `model.toml` (id, title, task,
   format, dimension, languages, licence, source, sha256 and size per
   file, layout version) and the files; the repository's catalogue
   (`tools/models/catalogue.toml`) is what packages are built from and
   what `models.fetch` reads. Packages may carry packs. A pack changes
   what the helper can answer, never what Skwir does.
5. **The one download.** `models.fetch {pack}`, and the Models page's
   Fetch, download a catalogued pack from its URL when the user asks,
   show progress, verify the hash, and are the one use of the network
   beside map tiles; nothing a file names is ever fetched. `AGENTS.md`'s
   network rule names it once built.
6. **What leaves the machine:** a remote provider is chosen per task in
   Settings (`models.remote.url`, `.name`, `.model`; the key in the
   keyring, typed by the user alone); while any task's provider leaves,
   the toolbar shows a persistent indicator named for the service, with
   the Models page and the log (`models.log`: when, to whom, how many
   bytes, which files by name, never the content) behind it. An agent's
   call that would leave is asked first; a private path is refused
   before anything is sent.
7. **Prompts and answers.** File names and contents are untrusted in
   every prompt, wrapped as data; instructions come from the plugin's
   source. An answer is data: a suggestion, a plan's step, a query's
   pills; it never runs a command (ADR-095).

## Consequences

- The `plugins_toggle` snapshot gains model providers; disabling the
  `models` plugin withdraws every provider and ends the helper.
- Ollama and the system's frameworks cost Skwir nothing; the helper
  costs a process while it runs and nothing after.
- The remote's key follows ADR-082's secrets: in the keyring, never a
  setting's value, never an argument.
- macOS and Windows providers are compiled only (`docs/PORTING.md`,
  `docs/HELP-WANTED.md`) until someone runs them.
- The candidate models (`docs/indexing-and-ai.md`, "Model packs") are
  measured by `skwir-bench embed` and `images` before any is the
  catalogue's default; licences are checked by `just deny` and the
  notices.
