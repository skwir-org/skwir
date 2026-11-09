//! The variables the exclusions' paths name (ADR-125): skwir's own
//! environment first, then what the user's login shell exports, which a
//! skwir started from a launcher does not inherit (`CARGO_HOME` set in
//! `.zshrc`). Core reads them; how the shell is asked is the platform's
//! (`skwir_platform::shell_environment`), installed as the asker.
//!
//! Only the names asked for are kept, never the rest of the shell's
//! environment: it holds tokens. What was learned changes the
//! [`generation`], which what is made of the rules keys on.

use std::collections::{BTreeSet, HashMap};
use std::ffi::OsString;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::RwLock;

/// What the shell said, and of which names.
#[derive(Default)]
struct Learned {
    values: HashMap<String, OsString>,
    asked: BTreeSet<String>,
}

static LEARNED: RwLock<Option<Learned>> = RwLock::new(None);
static GENERATION: AtomicU64 = AtomicU64::new(0);

type Asker = Box<dyn Fn(BTreeSet<String>) + Send + Sync>;
static ASKER: RwLock<Option<Asker>> = RwLock::new(None);

/// `name`'s value: skwir's own environment's, else the login shell's as
/// last learned. Empty is unset.
pub fn var(name: &str) -> Option<OsString> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            LEARNED
                .read()
                .as_ref()
                .and_then(|learned| learned.values.get(name).cloned())
        })
        .filter(|value| !value.is_empty())
}

/// Moves whenever what the shell said changes.
pub fn generation() -> u64 {
    GENERATION.load(Ordering::Acquire)
}

/// What the shell said of the names `asked`, replacing what was learned
/// before: the values of the names asked alone are kept. Whether anything
/// changed.
pub fn learn(
    asked: BTreeSet<String>,
    values: impl IntoIterator<Item = (String, OsString)>,
) -> bool {
    let values: HashMap<String, OsString> = values
        .into_iter()
        .filter(|(name, value)| asked.contains(name) && !value.is_empty())
        .collect();
    let mut learned = LEARNED.write();
    let changed = learned
        .as_ref()
        .is_none_or(|before| before.values != values || before.asked != asked);
    *learned = Some(Learned { values, asked });
    drop(learned);
    if changed {
        GENERATION.fetch_add(1, Ordering::AcqRel);
    }
    changed
}

/// Forget what the shell said: whether it is asked at all changed
/// (`exclusions.shell_variables`). Off, only skwir's own environment
/// counts; on again, the next [`want`] asks it.
pub fn forget() {
    if LEARNED.write().take().is_some() {
        GENERATION.fetch_add(1, Ordering::AcqRel);
    }
}

/// The names the shell was asked for.
pub fn asked() -> BTreeSet<String> {
    LEARNED
        .read()
        .as_ref()
        .map(|learned| learned.asked.clone())
        .unwrap_or_default()
}

/// Who asks the shell: handed the names to ask for, it asks on a worker
/// and answers through [`learn`]. None installed (tests, headless tools),
/// skwir's own environment is all there is.
pub fn set_asker(asker: impl Fn(BTreeSet<String>) + Send + Sync + 'static) {
    *ASKER.write() = Some(Box::new(asker));
}

/// The rules name `names`: those not asked for yet are asked, with those
/// asked before.
pub fn want(names: &BTreeSet<String>) {
    let before = asked();
    if names.is_subset(&before) {
        return;
    }
    let installed = ASKER.read();
    let Some(ask) = installed.as_ref() else {
        return;
    };
    let all: BTreeSet<String> = before.union(names).cloned().collect();
    // Remembered as asked at once, so a rule built before the answer does
    // not ask again; the answer replaces it.
    LEARNED
        .write()
        .get_or_insert_with(Learned::default)
        .asked
        .clone_from(&all);
    // The asker only hands the names to a worker.
    ask(all);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shells_values_stand_behind_skwirs_own_and_only_those_asked_are_kept() {
        let name = "SKWIR_TEST_ONLY_IN_THE_SHELL";
        assert_eq!(var(name), None);
        let before = generation();
        let asked = BTreeSet::from([name.to_owned()]);
        let said = HashMap::from([
            (name.to_owned(), OsString::from("/srv/it's ü")),
            ("SKWIR_TEST_A_TOKEN".to_owned(), OsString::from("secret")),
        ]);
        assert!(learn(asked.clone(), said.clone()));
        assert!(generation() > before);
        assert_eq!(var(name), Some(OsString::from("/srv/it's ü")));
        assert_eq!(var("SKWIR_TEST_A_TOKEN"), None, "never asked, never kept");
        let at = generation();
        assert!(!learn(asked, said), "the same answer changes nothing");
        assert_eq!(generation(), at);
        // skwir's own environment wins.
        assert!(var("PATH").is_some());

        // Forgotten, what depends on it is made again, and the names are
        // asked anew.
        forget();
        assert!(generation() > at);
        assert_eq!(var(name), None);
        assert!(super::asked().is_empty());
    }
}
