//! skwir plugin: checksums. A file's SHA-256, MD5 and BLAKE3 in Properties
//! (ADR-058), each computed only when asked: read on a worker, cancellable,
//! kept by the file's identity and version, so a file is read once until
//! it changes. Copy puts a checksum on the clipboard; Compare says whether
//! the one copied (a download page's) is the same.
//!
//! A script or an agent asks `checksums.compute`, which answers the
//! checksum where it is called; from the window, the checksum shows in
//! Properties when it is done.

use std::collections::HashMap;
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
use skwir_core::plugin::Plugin;
use skwir_core::registry::PropertySource;
use skwir_core::scheduler::Priority;

const ID: &str = "checksums";
/// Checksums kept, the oldest let go past it.
const KEPT: usize = 256;
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
        use sha2::Digest as _;
        enum Hasher {
            Sha256(Box<sha2::Sha256>),
            Md5(md5::Context),
            Blake3(Box<blake3::Hasher>),
        }
        let mut hasher = match self {
            Algorithm::Sha256 => Hasher::Sha256(Box::default()),
            Algorithm::Md5 => Hasher::Md5(md5::Context::new()),
            Algorithm::Blake3 => Hasher::Blake3(Box::default()),
        };
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
            match &mut hasher {
                Hasher::Sha256(h) => h.update(&buf[..n]),
                Hasher::Md5(h) => h.consume(&buf[..n]),
                Hasher::Blake3(h) => {
                    h.update(&buf[..n]);
                }
            }
        }
        Ok(Some(match hasher {
            Hasher::Sha256(h) => format!("{:x}", h.finalize()),
            Hasher::Md5(h) => format!("{:x}", h.finalize()),
            Hasher::Blake3(h) => h.finalize().to_hex().to_string(),
        }))
    }
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

#[derive(Default)]
struct State {
    /// Checksums computed, by file version and algorithm, oldest first.
    done: indexmap::IndexMap<(Version, Algorithm), String>,
    /// Being computed, each with what stops it.
    running: HashMap<(Version, Algorithm), CancelToken>,
}

/// The checksums plugin.
#[derive(Default)]
pub struct ChecksumsPlugin {
    state: Arc<Mutex<State>>,
    /// Properties' revision: moved when a checksum starts, ends or stops.
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

/// The one file named.
fn one(entries: Vec<EntryArg>) -> anyhow::Result<PathBuf> {
    let mut refs = EntryArg::refs(entries)?;
    anyhow::ensure!(refs.len() == 1, "name one file");
    Ok(refs.remove(0).path)
}

/// The checksum of the file at `path`, read whole; `None` once stopped.
fn compute(
    path: &Path,
    algorithm: Algorithm,
    cancel: &CancelToken,
) -> anyhow::Result<Option<String>> {
    let mut file = skwir_core::fs::file::open_regular(path)?;
    Ok(algorithm.digest(&mut file, cancel)?)
}

impl ChecksumsPlugin {
    fn changed(&self, app: &App) {
        self.revision.fetch_add(1, Ordering::Release);
        app.wake();
    }

    /// Keep `digest` for `version`, letting go of the oldest past `KEPT`.
    fn keep(&self, version: Version, algorithm: Algorithm, digest: String) {
        let mut state = self.state.lock();
        state.running.remove(&(version, algorithm));
        state.done.insert((version, algorithm), digest);
        while state.done.len() > KEPT {
            state.done.shift_remove_index(0);
        }
    }

    /// `checksums.properties`: the rows Properties shows for one file:
    /// each checksum computed, being computed, or the link that computes
    /// it.
    fn properties(&self, args: EntryArgs) -> anyhow::Result<Outcome> {
        let path = one(args.entries)?;
        let Some(version) = Version::of(&path) else {
            return Ok(Outcome::Value(serde_json::json!({ "rows": [] })));
        };
        let state = self.state.lock();
        let rows: Vec<serde_json::Value> = Algorithm::ALL
            .into_iter()
            .map(|algorithm| {
                let asked = serde_json::json!({ "entries": [path], "algorithm": algorithm });
                let running = state.running.contains_key(&(version, algorithm));
                let (value, actions) = if let Some(digest) = state.done.get(&(version, algorithm)) {
                    let said = serde_json::json!({ "text": digest, "what": algorithm.label() });
                    (
                        digest.clone(),
                        serde_json::json!([
                            { "words": "Copy", "command": { "name": "ui.copy_text", "args": said } },
                            { "words": "Compare with copied", "command": { "name": "ui.compare_copied", "args": said } },
                        ]),
                    )
                } else if running {
                    (
                        "Computing…".to_owned(),
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
                    "actions_shown": running,
                })
            })
            .collect();
        Ok(Outcome::Value(serde_json::json!({ "rows": rows })))
    }

    /// `checksums.compute`: from the window, on a worker, Properties saying
    /// it when done; anywhere else, read now and answered.
    fn start(self: &Arc<Self>, args: ComputeArgs, app: &App) -> anyhow::Result<Outcome> {
        let path = one(args.entries)?;
        let algorithm = args.algorithm;
        if !skwir_core::scheduler::on_window_thread() {
            let version = Version::of(&path)
                .ok_or_else(|| anyhow::anyhow!("{} is not a file", path.display()))?;
            if let Some(digest) = self.state.lock().done.get(&(version, algorithm)) {
                return Ok(answer(algorithm, digest));
            }
            let digest = compute(&path, algorithm, app.lifetime())?
                .ok_or_else(|| anyhow::anyhow!("stopped"))?;
            self.keep(version, algorithm, digest.clone());
            self.changed(app);
            return Ok(answer(algorithm, &digest));
        }
        let this = Arc::clone(self);
        let weak = app.weak();
        let cancel = app.lifetime().child();
        let job = cancel.clone();
        app.scheduler
            .spawn(Priority::Normal, "checksums", cancel, move || {
                let Some(app) = weak.upgrade() else {
                    return;
                };
                let Some(version) = Version::of(&path) else {
                    return;
                };
                {
                    let mut state = this.state.lock();
                    if state.done.contains_key(&(version, algorithm))
                        || state.running.contains_key(&(version, algorithm))
                    {
                        return;
                    }
                    state.running.insert((version, algorithm), job.clone());
                }
                this.changed(&app);
                match compute(&path, algorithm, &job) {
                    Ok(Some(digest)) => this.keep(version, algorithm, digest),
                    Ok(None) => {
                        this.state.lock().running.remove(&(version, algorithm));
                    }
                    Err(e) => {
                        this.state.lock().running.remove(&(version, algorithm));
                        app.notify(
                            skwir_core::app::NotifyLevel::Error,
                            format!("The {} could not be computed: {e:#}", algorithm.label()),
                        );
                    }
                }
                this.changed(&app);
            });
        Ok(Outcome::Done)
    }

    /// `checksums.stop`: stop computing a checksum.
    fn stop(&self, args: ComputeArgs, app: &App) -> anyhow::Result<Outcome> {
        let path = one(args.entries)?;
        let stopped: Vec<CancelToken> = {
            let mut state = self.state.lock();
            let keys: Vec<(Version, Algorithm)> = state
                .running
                .keys()
                .filter(|(_, a)| *a == args.algorithm)
                .copied()
                .collect();
            // By version where the file says it; a file gone stops all.
            let version = Version::of(&path);
            keys.into_iter()
                .filter(|(v, _)| version.is_none_or(|now| now == *v))
                .filter_map(|key| state.running.remove(&key))
                .collect()
        };
        for token in stopped {
            token.cancel();
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
            .reads();
        let t = Arc::clone(&this);
        app.commands
            .register::<ComputeArgs, _>(
                ID,
                "checksums.compute",
                "Compute a checksum",
                "The `sha256`, `md5` or `blake3` checksum of a file, read whole: answered where it is called, or from the window shown in Properties when done",
                move |a, app| t.start(a, app),
            )
            .reads();
        let t = Arc::clone(&this);
        app.commands
            .register::<ComputeArgs, _>(
                ID,
                "checksums.stop",
                "Stop computing a checksum",
                "Stop computing a file's checksum",
                move |a, app| t.stop(a, app),
            )
            .reads();
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
        for (_, token) in state.running.drain() {
            token.cancel();
        }
        state.done.clear();
        self.revision.fetch_add(1, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skwir_core::app::AppConfig;

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
        let stopped = CancelToken::new();
        stopped.cancel();
        assert_eq!(
            Algorithm::Md5.digest(&mut &big[..], &stopped).unwrap(),
            None
        );
    }

    #[test]
    fn properties_compute_when_asked_and_keep_by_version() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("it's a café.iso");
        std::fs::write(&file, b"abc").unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        let plugin = Arc::new(ChecksumsPlugin::default());
        app.plugins.add(plugin.clone(), &app);
        let rows = |app: &App| {
            let Outcome::Value(answer) = app
                .dispatch(&Invocation::with_args(
                    "checksums.properties",
                    &serde_json::json!({ "entries": [file] }),
                ))
                .unwrap()
            else {
                panic!("rows are a value")
            };
            answer["rows"].clone()
        };
        let before = rows(&app);
        assert_eq!(before[0]["label"], "SHA-256");
        assert_eq!(before[0]["value"], "");
        assert_eq!(before[0]["actions"][0]["words"], "Compute");
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
        let after = rows(&app);
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
        assert_eq!(rows(&app)[0]["value"], "");
        // A folder has none.
        let Outcome::Value(folder) = app
            .dispatch(&Invocation::with_args(
                "checksums.properties",
                &serde_json::json!({ "entries": [tmp.path()] }),
            ))
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(folder["rows"], serde_json::json!([]));
        app.plugins.set_enabled(ID, false, &app);
        assert!(
            plugin.state.lock().done.is_empty(),
            "disabled, it keeps nothing"
        );
    }
}
