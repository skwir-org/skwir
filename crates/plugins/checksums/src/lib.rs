//! skwir plugin: checksums. A file's SHA-256, MD5 and BLAKE3 in Properties
//! (ADR-058), each computed only when asked, kept by the file's identity
//! and version, so a file is read once until it changes. Copy puts a
//! checksum on the clipboard; Compare says whether the one copied (a
//! download page's) is the same.
//!
//! A script or an agent asks `checksums.compute`, which answers the
//! checksum where it is called. From the window, what is asked waits in a
//! lane: one job at a time reads the file asked last, for every checksum
//! asked of it at once, so a large file holds one worker, never all of
//! them while folders wait to be listed (AGENTS.md, principle 1). The
//! window names the file as Properties shows it and nothing is read on its
//! thread; the checksum shows in Properties when it is done, and only if
//! the file did not change while it was read.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use parking_lot::Mutex;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skwir_core::FileId;
use skwir_core::app::App;
use skwir_core::cancel::CancelToken;
use skwir_core::command::{Invocation, Outcome};
use skwir_core::op::EntryArg;
use skwir_core::os_path::PathArg;
use skwir_core::plugin::Plugin;
use skwir_core::registry::PropertySource;
use skwir_core::scheduler::Priority;

const ID: &str = "checksums";
/// Checksums kept, the oldest let go past it.
const KEPT: usize = 256;
/// Files waiting for the lane, at most: past it the oldest asked is let
/// go, and Properties offers to compute it again.
const ASKED_AT_MOST: usize = 16;
/// What is read at a time: cancelling answers within one.
const CHUNK: usize = 1 << 20;

/// A kind of checksum.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Algorithm {
    Sha256,
    Md5,
    Blake3,
}

/// A checksum being computed.
enum Hasher {
    Sha256(Box<sha2::Sha256>),
    Md5(md5::Context),
    Blake3(Box<blake3::Hasher>),
}

impl Hasher {
    fn new(algorithm: Algorithm) -> Self {
        match algorithm {
            Algorithm::Sha256 => Hasher::Sha256(Box::default()),
            Algorithm::Md5 => Hasher::Md5(md5::Context::new()),
            Algorithm::Blake3 => Hasher::Blake3(Box::default()),
        }
    }

    fn update(&mut self, bytes: &[u8]) {
        use sha2::Digest as _;
        match self {
            Hasher::Sha256(h) => h.update(bytes),
            Hasher::Md5(h) => h.consume(bytes),
            Hasher::Blake3(h) => {
                h.update(bytes);
            }
        }
    }

    /// In lowercase hexadecimal.
    fn finish(self) -> String {
        use sha2::Digest as _;
        match self {
            Hasher::Sha256(h) => format!("{:x}", h.finalize()),
            Hasher::Md5(h) => format!("{:x}", h.finalize()),
            Hasher::Blake3(h) => h.finalize().to_hex().to_string(),
        }
    }
}

impl Algorithm {
    /// In the order Properties lists them: the one download pages give
    /// first.
    pub const ALL: [Algorithm; 3] = [Algorithm::Sha256, Algorithm::Md5, Algorithm::Blake3];

    pub fn label(self) -> &'static str {
        match self {
            Algorithm::Sha256 => "SHA-256",
            Algorithm::Md5 => "MD5",
            Algorithm::Blake3 => "BLAKE3",
        }
    }

    /// The checksum of what `from` holds, in lowercase hexadecimal; `None`
    /// once `cancel` stops it.
    ///
    /// # Errors
    /// When the file cannot be read.
    pub fn digest(
        self,
        from: &mut impl Read,
        cancel: &CancelToken,
    ) -> std::io::Result<Option<String>> {
        Ok(digests(&[self], from, cancel)?.map(|mut all| all.remove(0)))
    }
}

/// The checksums `algorithms` of what `from` holds, in their order, from
/// one read; `None` once `cancel` stops it.
///
/// # Errors
/// When the file cannot be read.
pub fn digests(
    algorithms: &[Algorithm],
    from: &mut impl Read,
    cancel: &CancelToken,
) -> std::io::Result<Option<Vec<String>>> {
    let mut hashers: Vec<Hasher> = algorithms.iter().map(|a| Hasher::new(*a)).collect();
    let mut buf = vec![0; CHUNK];
    loop {
        if cancel.is_cancelled() {
            return Ok(None);
        }
        let n = match from.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        for hasher in &mut hashers {
            hasher.update(&buf[..n]);
        }
    }
    Ok(Some(hashers.into_iter().map(Hasher::finish).collect()))
}

/// A file as it is: what its checksums are kept by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Version {
    id: FileId,
    size: u64,
    modified: Option<SystemTime>,
}

impl Version {
    /// The version of the regular file `path` leads to; `None` for
    /// anything else (a folder, a pipe, a device). Stats it: on a worker.
    fn of(path: &Path) -> Option<Version> {
        let meta = std::fs::metadata(path)
            .ok()
            .filter(std::fs::Metadata::is_file)?;
        Some(Version {
            id: skwir_core::fs::stat::identity(path, &meta)?,
            size: meta.len(),
            modified: meta.modified().ok(),
        })
    }
}

/// What a read of a file said.
#[derive(Debug, PartialEq)]
enum Hashed {
    /// The checksums of `Version`, in the order asked.
    Done(Version, Vec<String>),
    /// Stopped before the end.
    Stopped,
    /// The file changed while it was read: what was read is no version.
    Changed,
}

/// The checksums `algorithms` of the file at `path`, in one read, if it is
/// still `before` once read: stat-ed again after, and the file read seen
/// again, so a file written meanwhile gives no checksum of a version it
/// never was. Reads the disk: on a worker, or where a script waits.
fn read(
    path: &Path,
    before: Version,
    algorithms: &[Algorithm],
    cancel: &CancelToken,
) -> anyhow::Result<Hashed> {
    let mut file = skwir_core::fs::file::open_regular(path)?;
    let Some(digests) = digests(algorithms, &mut file, cancel)? else {
        return Ok(Hashed::Stopped);
    };
    let read = file.metadata()?;
    let same = Version::of(path) == Some(before)
        && read.len() == before.size
        && read.modified().ok() == before.modified;
    Ok(if same {
        Hashed::Done(before, digests)
    } else {
        Hashed::Changed
    })
}

/// A file as the window names it, read from no disk: its absolute path,
/// and its identity where the window knew it (Properties' rows say it).
#[derive(Clone, Debug, PartialEq, Eq)]
struct Named {
    path: PathBuf,
    id: Option<FileId>,
}

impl Named {
    /// The one file `entries` name.
    fn of(mut entries: Vec<EntryArg>) -> anyhow::Result<Named> {
        anyhow::ensure!(entries.len() == 1, "name one file");
        let (id, path) = match entries.remove(0) {
            EntryArg::Path(path) => (None, path.into_inner()),
            EntryArg::Named { id, path } => (id, path.into_inner()),
        };
        skwir_core::os_path::absolute(&path)?;
        Ok(Named { path, id })
    }

    /// Whether it names the same file as `other`: the same path, and the
    /// same identity where both know it.
    fn is(&self, other: &Named) -> bool {
        self.path == other.path && (self.id.is_none() || other.id.is_none() || self.id == other.id)
    }

    /// As a command's `entries` say it: lossless, whatever bytes its name
    /// holds.
    fn arg(&self) -> EntryArg {
        EntryArg::Named {
            id: self.id,
            path: PathArg(self.path.clone()),
        }
    }
}

/// A file the window asked checksums of, waiting for the lane.
struct Ask {
    file: Named,
    algorithms: Vec<Algorithm>,
}

/// The file the lane reads.
struct Reading {
    file: Named,
    /// What is still wanted of it: one stopped is taken out, and none left
    /// stops the read.
    algorithms: Vec<Algorithm>,
    cancel: CancelToken,
}

#[derive(Default)]
struct State {
    /// Checksums computed, by file version and algorithm, oldest first.
    done: indexmap::IndexMap<(Version, Algorithm), String>,
    /// Asked from the window, the last asked last: each file once.
    asked: Vec<Ask>,
    /// What the lane reads now.
    reading: Option<Reading>,
    /// The lane's job, while one holds a worker or waits for one: never
    /// two at a time.
    lane: Option<u64>,
    lanes: u64,
    /// What the plugin's work is a child of, while it is on: turning it
    /// off cancels what runs and what waits for a worker alike.
    lifetime: Option<CancelToken>,
    /// Files read by the lane: each once for all that is asked of it.
    #[cfg(test)]
    reads: usize,
}

impl State {
    /// Whether `algorithm` of `file` waits for the lane (`Some(false)`) or
    /// is being read (`Some(true)`).
    fn computing(&self, file: &Named, algorithm: Algorithm) -> Option<bool> {
        if self
            .reading
            .as_ref()
            .is_some_and(|r| r.file.is(file) && r.algorithms.contains(&algorithm))
        {
            return Some(true);
        }
        self.asked
            .iter()
            .any(|ask| ask.file.is(file) && ask.algorithms.contains(&algorithm))
            .then_some(false)
    }
}

/// The checksums plugin.
#[derive(Default)]
pub struct ChecksumsPlugin {
    state: Arc<Mutex<State>>,
    /// Properties' revision: moved when a checksum is asked, starts, ends
    /// or stops.
    revision: Arc<AtomicU64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct EntryArgs {
    /// The file, by absolute path.
    entries: Vec<EntryArg>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ComputeArgs {
    /// The file, by absolute path.
    entries: Vec<EntryArg>,
    /// Which checksum: `sha256`, `md5` or `blake3`.
    algorithm: Algorithm,
}

impl ChecksumsPlugin {
    fn changed(&self, app: &App) {
        self.revision.fetch_add(1, Ordering::Release);
        app.wake();
    }

    /// Keep `digest` for `version`, letting go of the oldest past `KEPT`.
    fn keep(state: &mut State, version: Version, algorithm: Algorithm, digest: String) {
        state.done.insert((version, algorithm), digest);
        while state.done.len() > KEPT {
            state.done.shift_remove_index(0);
        }
    }

    /// `checksums.properties`: the rows Properties shows for one file:
    /// each checksum computed, being computed or waiting, or the link that
    /// computes it. Their group ships folded. Stats the file: Properties
    /// asks on a worker.
    fn properties(&self, args: EntryArgs) -> anyhow::Result<Outcome> {
        let file = Named::of(args.entries)?;
        let Some(version) = Version::of(&file.path) else {
            return Ok(Outcome::Value(serde_json::json!({ "rows": [] })));
        };
        let entries = [file.arg()];
        let state = self.state.lock();
        let rows: Vec<serde_json::Value> = Algorithm::ALL
            .into_iter()
            .map(|algorithm| {
                let asked = serde_json::json!({ "entries": entries, "algorithm": algorithm });
                let computing = state.computing(&file, algorithm);
                let (value, actions) = if let Some(digest) = state.done.get(&(version, algorithm)) {
                    let said = serde_json::json!({ "text": digest, "what": algorithm.label() });
                    (
                        digest.clone(),
                        serde_json::json!([
                            { "words": "Copy", "command": { "name": "ui.copy_text", "args": said } },
                            { "words": "Compare with copied", "command": { "name": "ui.compare_copied", "args": said } },
                        ]),
                    )
                } else if let Some(reading) = computing {
                    (
                        if reading { "Computing…" } else { "Waiting…" }.to_owned(),
                        serde_json::json!([
                            { "words": "Stop", "command": { "name": "checksums.stop", "args": asked } },
                        ]),
                    )
                } else {
                    (
                        String::new(),
                        serde_json::json!([
                            { "words": "Compute", "command": { "name": "checksums.compute", "args": asked } },
                        ]),
                    )
                };
                serde_json::json!({
                    "group": "Checksums",
                    "label": algorithm.label(),
                    "value": value,
                    "actions": actions,
                    // Stop shows without the pointer on the row.
                    "actions_shown": computing.is_some(),
                    // Seldom looked at: the group ships folded.
                    "folded": true,
                })
            })
            .collect();
        Ok(Outcome::Value(serde_json::json!({ "rows": rows })))
    }

    /// `checksums.compute`: from the window, in the lane, Properties saying
    /// it when done; anywhere else, read now and answered.
    fn start(self: &Arc<Self>, args: ComputeArgs, app: &App) -> anyhow::Result<Outcome> {
        let algorithm = args.algorithm;
        if !skwir_core::scheduler::on_window_thread() {
            // A script's or an agent's thread: it waits here.
            let file = Named::of(args.entries)?;
            let version = Version::of(&file.path)
                .ok_or_else(|| anyhow::anyhow!("{} is not a file", file.path.display()))?;
            if let Some(digest) = self.state.lock().done.get(&(version, algorithm)) {
                return Ok(answer(algorithm, digest));
            }
            let digest = match read(&file.path, version, &[algorithm], app.lifetime())? {
                Hashed::Done(_, mut digests) => digests.remove(0),
                Hashed::Stopped => anyhow::bail!("stopped"),
                Hashed::Changed => anyhow::bail!(
                    "{} changed while it was read: ask again",
                    file.path.display()
                ),
            };
            Self::keep(&mut self.state.lock(), version, algorithm, digest.clone());
            self.changed(app);
            return Ok(answer(algorithm, &digest));
        }
        self.ask(Named::of(args.entries)?, algorithm, app);
        Ok(Outcome::Done)
    }

    /// Ask `algorithm` of `file` in the lane: with what is asked of it
    /// already (one read for all), as the last asked, read first. Reads
    /// nothing here (the window's thread).
    fn ask(self: &Arc<Self>, file: Named, algorithm: Algorithm, app: &App) {
        let lane = {
            let mut state = self.state.lock();
            let Some(lifetime) = state.lifetime.clone() else {
                return; // turned off
            };
            if state.computing(&file, algorithm) == Some(true) {
                return;
            }
            let ask = match state.asked.iter().position(|ask| ask.file.is(&file)) {
                Some(at) => {
                    let mut ask = state.asked.remove(at);
                    if !ask.algorithms.contains(&algorithm) {
                        ask.algorithms.push(algorithm);
                    }
                    ask
                }
                None => Ask {
                    file,
                    algorithms: vec![algorithm],
                },
            };
            state.asked.push(ask);
            if state.asked.len() > ASKED_AT_MOST {
                state.asked.remove(0);
            }
            if state.lane.is_some() {
                None
            } else {
                state.lanes += 1;
                state.lane = Some(state.lanes);
                Some((state.lanes, lifetime))
            }
        };
        self.changed(app);
        if let Some((lane, lifetime)) = lane {
            let this = Arc::clone(self);
            let weak = app.weak();
            let job = lifetime.child();
            app.scheduler
                .spawn(Priority::Normal, "checksums", job, move || {
                    if let Some(app) = weak.upgrade() {
                        while this.read_next(&app, lane, &lifetime) {}
                    }
                    let mut state = this.state.lock();
                    if state.lane == Some(lane) {
                        state.lane = None;
                    }
                });
        }
    }

    /// The lane's turn: read the file asked last, for what is asked of it;
    /// false once nothing waits or the plugin was turned off.
    fn read_next(&self, app: &App, lane: u64, lifetime: &CancelToken) -> bool {
        let (file, cancel) = {
            let mut state = self.state.lock();
            if lifetime.is_cancelled() || state.lane != Some(lane) {
                return false;
            }
            let Some(ask) = state.asked.pop() else {
                state.lane = None;
                return false;
            };
            let cancel = lifetime.child();
            state.reading = Some(Reading {
                file: ask.file.clone(),
                algorithms: ask.algorithms,
                cancel: cancel.clone(),
            });
            (ask.file, cancel)
        };
        self.changed(app);
        let read = match Version::of(&file.path) {
            Some(version) => {
                // What is kept for this version already is not read again.
                let wanted: Vec<Algorithm> = {
                    let state = self.state.lock();
                    state
                        .reading
                        .as_ref()
                        .filter(|_| state.lane == Some(lane))
                        .map(|r| r.algorithms.clone())
                        .unwrap_or_default()
                        .into_iter()
                        .filter(|a| !state.done.contains_key(&(version, *a)))
                        .collect()
                };
                #[cfg(test)]
                {
                    self.state.lock().reads += usize::from(!wanted.is_empty());
                }
                if wanted.is_empty() {
                    Ok((wanted, Hashed::Stopped))
                } else {
                    read(&file.path, version, &wanted, &cancel).map(|read| (wanted, read))
                }
            }
            None => Err(anyhow::anyhow!("it is not a file")),
        };
        let name = file.path.file_name().map_or_else(
            || file.path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        {
            let mut state = self.state.lock();
            // What is still wanted once read: Stop takes one out. Turned
            // off (and maybe on again) meanwhile, what is read is another
            // lane's business now, and nothing is kept.
            let still = if state.lane == Some(lane) {
                state
                    .reading
                    .take()
                    .map(|r| r.algorithms)
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
            match read {
                Ok((read_for, Hashed::Done(version, digests))) => {
                    for (algorithm, digest) in read_for.into_iter().zip(digests) {
                        if still.contains(&algorithm) {
                            Self::keep(&mut state, version, algorithm, digest);
                        }
                    }
                }
                Ok((_, Hashed::Changed)) => app.notify(
                    skwir_core::app::NotifyLevel::Warning,
                    format!("“{name}” changed while its checksum was computed: compute it again"),
                ),
                Err(e) if !cancel.is_cancelled() => app.notify(
                    skwir_core::app::NotifyLevel::Error,
                    format!("The checksum of “{name}” could not be computed: {e:#}"),
                ),
                // Stopped, or failing because it was.
                Ok((_, Hashed::Stopped)) | Err(_) => {}
            }
        }
        self.changed(app);
        true
    }

    /// `checksums.stop`: stop computing a checksum, waiting or being read.
    /// The window names the file as it asked it: nothing is read here.
    fn stop(&self, args: ComputeArgs, app: &App) -> anyhow::Result<Outcome> {
        let file = Named::of(args.entries)?;
        {
            let mut state = self.state.lock();
            for ask in state.asked.iter_mut().filter(|ask| ask.file.is(&file)) {
                ask.algorithms.retain(|a| *a != args.algorithm);
            }
            state.asked.retain(|ask| !ask.algorithms.is_empty());
            if let Some(reading) = state.reading.as_mut().filter(|r| r.file.is(&file)) {
                reading.algorithms.retain(|a| *a != args.algorithm);
                if reading.algorithms.is_empty() {
                    reading.cancel.cancel();
                }
            }
        }
        self.changed(app);
        Ok(Outcome::Done)
    }
}

fn answer(algorithm: Algorithm, digest: &str) -> Outcome {
    Outcome::Value(serde_json::json!({ "algorithm": algorithm, "checksum": digest }))
}

impl Plugin for ChecksumsPlugin {
    fn id(&self) -> &'static str {
        ID
    }

    fn title(&self) -> &'static str {
        "Checksums"
    }

    fn description(&self) -> &'static str {
        "A file's SHA-256, MD5 and BLAKE3 in Properties, computed when asked, and compared with one copied"
    }

    fn category(&self) -> skwir_core::plugin::Category {
        skwir_core::plugin::Category::Files
    }

    fn capabilities(&self) -> skwir_core::permissions::Capabilities {
        use skwir_core::permissions::Capability;
        skwir_core::permissions::Capabilities::of(&[Capability::Read])
    }

    fn register(&self, app: &App) {
        {
            let mut state = self.state.lock();
            if let Some(before) = state.lifetime.replace(app.lifetime().child()) {
                before.cancel();
            }
        }
        let this = Arc::new(ChecksumsPlugin {
            state: Arc::clone(&self.state),
            revision: Arc::clone(&self.revision),
        });
        let t = Arc::clone(&this);
        app.commands
            .register::<EntryArgs, _>(
                ID,
                "checksums.properties",
                "Checksums in Properties",
                "Properties' rows for one file: each checksum computed, being computed, or the way to compute it",
                move |a, _| t.properties(a),
            )
            .data();
        let t = Arc::clone(&this);
        app.commands
            .register::<ComputeArgs, _>(
                ID,
                "checksums.compute",
                "Compute a checksum",
                "The `sha256`, `md5` or `blake3` checksum of a file, read whole: answered where it is called, or from the window shown in Properties when done",
                move |a, app| t.start(a, app),
            )
            .data();
        let t = Arc::clone(&this);
        app.commands
            .register::<ComputeArgs, _>(
                ID,
                "checksums.stop",
                "Stop computing a checksum",
                "Stop computing a file's checksum",
                move |a, app| t.stop(a, app),
            )
            .data();
        app.registry.property_sources.add(
            ID,
            PropertySource {
                command: Invocation::new("checksums.properties"),
                revision: Arc::clone(&self.revision),
            },
        );
    }

    fn on_disable(&self, _app: &App) {
        let mut state = self.state.lock();
        // What runs and what waits for a worker alike: the lane's job is a
        // child of it, and the scheduler skips a job cancelled before it
        // starts.
        if let Some(lifetime) = state.lifetime.take() {
            lifetime.cancel();
        }
        state.asked.clear();
        state.reading = None;
        state.lane = None;
        state.done.clear();
        self.revision.fetch_add(1, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skwir_core::app::AppConfig;

    /// A folder under the tests' own cache folder, on the home drive.
    fn fixture() -> tempfile::TempDir {
        let base =
            PathBuf::from(std::env::var_os("HOME").expect("a home")).join(".cache/skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        tempfile::tempdir_in(base).unwrap()
    }

    #[test]
    fn checksums_are_the_known_ones() {
        let cancel = CancelToken::new();
        let digest =
            |a: Algorithm, bytes: &[u8]| a.digest(&mut &bytes[..], &cancel).unwrap().unwrap();
        assert_eq!(
            digest(Algorithm::Sha256, b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            digest(Algorithm::Md5, b"abc"),
            "900150983cd24fb0d6963f7d28e17f72"
        );
        assert_eq!(
            digest(Algorithm::Blake3, b""),
            "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262"
        );
        // Larger than a chunk: the same as at once.
        let big = vec![7u8; CHUNK * 2 + 3];
        let mut whole = sha2::Sha256::default();
        sha2::Digest::update(&mut whole, &big);
        assert_eq!(
            digest(Algorithm::Sha256, &big),
            format!("{:x}", sha2::Digest::finalize(whole))
        );
        // Several from one read: each as alone.
        let all = digests(&Algorithm::ALL, &mut &big[..], &cancel)
            .unwrap()
            .unwrap();
        let alone: Vec<String> = Algorithm::ALL.iter().map(|a| digest(*a, &big)).collect();
        assert_eq!(all, alone);
        let stopped = CancelToken::new();
        stopped.cancel();
        assert_eq!(
            Algorithm::Md5.digest(&mut &big[..], &stopped).unwrap(),
            None
        );
    }

    fn rows(app: &App, entries: &serde_json::Value) -> serde_json::Value {
        let Outcome::Value(answer) = app
            .dispatch(&Invocation::with_args(
                "checksums.properties",
                &serde_json::json!({ "entries": entries }),
            ))
            .unwrap()
        else {
            panic!("rows are a value")
        };
        answer["rows"].clone()
    }

    #[test]
    fn properties_compute_when_asked_and_keep_by_version() {
        let tmp = fixture();
        let file = tmp.path().join("it's a café.iso");
        std::fs::write(&file, b"abc").unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        let plugin = Arc::new(ChecksumsPlugin::default());
        app.plugins.add(plugin.clone(), &app);
        let entries = serde_json::json!([file]);
        let before = rows(&app, &entries);
        assert_eq!(before[0]["label"], "SHA-256");
        assert_eq!(before[0]["value"], "");
        assert_eq!(before[0]["actions"][0]["words"], "Compute");
        assert_eq!(before[0]["folded"], true, "the group ships folded");
        // Off the window's thread, the command answers.
        let Outcome::Value(done) = app
            .dispatch(&Invocation::with_args(
                "checksums.compute",
                &serde_json::json!({ "entries": [file], "algorithm": "sha256" }),
            ))
            .unwrap()
        else {
            panic!("a checksum is a value")
        };
        assert_eq!(
            done["checksum"],
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let after = rows(&app, &entries);
        assert_eq!(after[0]["value"], done["checksum"]);
        let words: Vec<&str> = after[0]["actions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["words"].as_str().unwrap())
            .collect();
        assert_eq!(words, ["Copy", "Compare with copied"]);
        assert_eq!(after[1]["value"], "", "each when asked");
        // A file changed is another version: computed again when asked.
        std::fs::write(&file, b"abcd").unwrap();
        assert_eq!(rows(&app, &entries)[0]["value"], "");
        // A folder has none.
        assert_eq!(
            rows(&app, &serde_json::json!([tmp.path()])),
            serde_json::json!([])
        );
        app.plugins.set_enabled(ID, false, &app);
        assert!(
            plugin.state.lock().done.is_empty(),
            "disabled, it keeps nothing"
        );
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn a_name_that_is_not_utf8_gets_its_rows_and_its_checksum() {
        use std::os::unix::ffi::OsStringExt;
        let tmp = fixture();
        let file = tmp
            .path()
            .join(std::ffi::OsString::from_vec(b"caf\xe9 \xff.iso".to_vec()));
        std::fs::write(&file, b"abc").unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        app.plugins.add(Arc::new(ChecksumsPlugin::default()), &app);
        // As Properties names it: an `EntryRef`, its path in bytes.
        let named = skwir_core::op::EntryRef::from_path(&file).unwrap();
        let rows = rows(&app, &serde_json::json!([named]));
        assert_eq!(rows[0]["actions"][0]["words"], "Compute");
        // The link's arguments name the same file, whatever its bytes.
        let link = &rows[0]["actions"][0]["command"];
        let Outcome::Value(done) = app
            .dispatch(&Invocation::with_args(
                link["name"].as_str().unwrap(),
                &link["args"],
            ))
            .unwrap()
        else {
            panic!("a checksum is a value")
        };
        assert_eq!(
            done["checksum"],
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn a_file_changed_while_read_gives_no_checksum() {
        let tmp = fixture();
        let file = tmp.path().join("growing log");
        std::fs::write(&file, b"abc").unwrap();
        let before = Version::of(&file).unwrap();
        let cancel = CancelToken::new();
        assert!(matches!(
            read(&file, before, &[Algorithm::Md5], &cancel).unwrap(),
            Hashed::Done(v, _) if v == before
        ));
        // Written since its version was taken: what is read is not it.
        std::fs::write(&file, b"abc and more").unwrap();
        assert_eq!(
            read(&file, before, &[Algorithm::Md5], &cancel).unwrap(),
            Hashed::Changed
        );
    }

    /// An app of one worker, and a job holding it until `release` is sent:
    /// what is spawned meanwhile waits for it.
    fn one_held_worker(root: &Path) -> (Arc<App>, std::sync::mpsc::Sender<()>) {
        let app = App::new(AppConfig {
            workers: 1,
            ..AppConfig::ephemeral(root)
        })
        .unwrap();
        let (release, held) = std::sync::mpsc::channel::<()>();
        let (started, running) = std::sync::mpsc::channel::<()>();
        app.scheduler
            .spawn(Priority::Visible, "held", CancelToken::new(), move || {
                started.send(()).unwrap();
                let _ = held.recv();
            });
        running.recv().unwrap();
        (app, release)
    }

    /// Dispatch on a thread marked as the window's, as the window's links
    /// do, and answer what it answered.
    fn from_window(app: &Arc<App>, name: &'static str, args: serde_json::Value) -> Outcome {
        let app = Arc::clone(app);
        std::thread::spawn(move || {
            skwir_core::scheduler::mark_window_thread(&app.scheduler);
            app.dispatch(&Invocation::with_args(name, &args)).unwrap()
        })
        .join()
        .unwrap()
    }

    fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while !done() {
            assert!(std::time::Instant::now() < deadline, "{what}: timed out");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn the_window_asks_in_one_lane_and_one_read_per_file() {
        let tmp = fixture();
        let (a, b) = (tmp.path().join("a.iso"), tmp.path().join("b café.iso"));
        std::fs::write(&a, b"abc").unwrap();
        std::fs::write(&b, b"").unwrap();
        let (app, release) = one_held_worker(tmp.path());
        let plugin = Arc::new(ChecksumsPlugin::default());
        app.plugins.add(plugin.clone(), &app);
        let ask = |file: &Path, algorithm: &str| {
            let named = skwir_core::op::EntryRef::from_path(file).unwrap();
            from_window(
                &app,
                "checksums.compute",
                serde_json::json!({ "entries": [named], "algorithm": algorithm }),
            )
        };
        // Asked from the window: answered at once, read later. A file that
        // is not there is not looked at on the window's thread either.
        assert!(matches!(ask(&a, "sha256"), Outcome::Done));
        assert!(matches!(ask(&a, "md5"), Outcome::Done));
        assert!(matches!(ask(&b, "blake3"), Outcome::Done));
        assert!(matches!(
            from_window(
                &app,
                "checksums.compute",
                serde_json::json!({ "entries": [tmp.path().join("not there")], "algorithm": "md5" }),
            ),
            Outcome::Done
        ));
        {
            let state = plugin.state.lock();
            assert_eq!(state.asked.len(), 3, "each file once");
            assert_eq!(
                state.asked[0].algorithms,
                [Algorithm::Sha256, Algorithm::Md5]
            );
            assert!(state.lane.is_some());
        }
        let entries = serde_json::json!([skwir_core::op::EntryRef::from_path(&a).unwrap()]);
        assert_eq!(rows(&app, &entries)[0]["value"], "Waiting…");
        assert_eq!(rows(&app, &entries)[0]["actions"][0]["words"], "Stop");
        release.send(()).unwrap();
        wait_for("the lane", || {
            let state = plugin.state.lock();
            state.lane.is_none() && state.asked.is_empty()
        });
        let rows = rows(&app, &entries);
        assert_eq!(
            rows[0]["value"],
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(rows[1]["value"], "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(plugin.state.lock().reads, 2, "a once for both, b once");
        assert!(app.scheduler.wait_idle(std::time::Duration::from_secs(20)));
        let lanes = app
            .scheduler
            .stats()
            .into_iter()
            .find(|(label, _)| *label == "checksums")
            .map_or(0, |(_, stats)| stats.runs);
        assert_eq!(lanes, 1, "one job held a worker for them all");
        // The file not there said so, as a message.
        assert!(
            app.take_notifications()
                .iter()
                .any(|n| n.text.contains("not there")),
            "the missing file is said"
        );
    }

    #[test]
    fn turned_off_what_waits_for_a_worker_never_runs() {
        let tmp = fixture();
        let file = tmp.path().join("big one.iso");
        std::fs::write(&file, b"abc").unwrap();
        let (app, release) = one_held_worker(tmp.path());
        let plugin = Arc::new(ChecksumsPlugin::default());
        app.plugins.add(plugin.clone(), &app);
        let named = skwir_core::op::EntryRef::from_path(&file).unwrap();
        from_window(
            &app,
            "checksums.compute",
            serde_json::json!({ "entries": [named], "algorithm": "sha256" }),
        );
        app.plugins.set_enabled(ID, false, &app);
        release.send(()).unwrap();
        // The worker is free again once the held job ends: by then the
        // lane's job, cancelled, was skipped.
        wait_for("the held job", || {
            app.scheduler
                .stats()
                .iter()
                .any(|(label, stats)| *label == "held" && stats.runs == 1)
        });
        assert!(app.scheduler.wait_idle(std::time::Duration::from_secs(20)));
        assert_eq!(plugin.state.lock().reads, 0, "nothing read once off");
        assert!(plugin.state.lock().done.is_empty());
        // On again: it computes when asked, in a lane of its own.
        app.plugins.set_enabled(ID, true, &app);
        from_window(
            &app,
            "checksums.compute",
            serde_json::json!({ "entries": [named], "algorithm": "sha256" }),
        );
        wait_for("the lane", || !plugin.state.lock().done.is_empty());
    }

    #[test]
    fn stop_takes_one_checksum_out_of_what_waits() {
        let tmp = fixture();
        let file = tmp.path().join("a.iso");
        std::fs::write(&file, b"abc").unwrap();
        let (app, release) = one_held_worker(tmp.path());
        let plugin = Arc::new(ChecksumsPlugin::default());
        app.plugins.add(plugin.clone(), &app);
        let named = skwir_core::op::EntryRef::from_path(&file).unwrap();
        for algorithm in ["sha256", "md5"] {
            from_window(
                &app,
                "checksums.compute",
                serde_json::json!({ "entries": [named], "algorithm": algorithm }),
            );
        }
        from_window(
            &app,
            "checksums.stop",
            serde_json::json!({ "entries": [named], "algorithm": "sha256" }),
        );
        release.send(()).unwrap();
        wait_for("the lane", || plugin.state.lock().lane.is_none());
        let rows = rows(&app, &serde_json::json!([named]));
        assert_eq!(rows[0]["value"], "", "SHA-256 was stopped");
        assert_eq!(rows[1]["value"], "900150983cd24fb0d6963f7d28e17f72");
    }
}
