//! Hints: where files that may match a search are, from an index that
//! answers faster than a walk (ADR-026). The system's (Baloo) or one a
//! plugin keeps in the cache.
//!
//! A hint is never the result. A search considers the files hinted first,
//! each checked against the file as any other is, then walks as it would
//! have, skipping those it already considered: an index that is out of
//! date costs a few checks, never a wrong or a missing answer, and no
//! search waits for one to be built.
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::registry::{Contributions, OwnerGate};

use crate::cancel::CancelToken;

/// Answers, from an index, which files may have a value in a field.
pub trait HintProvider: Send + Sync {
    fn id(&self) -> &'static str;
    /// Files under `root` that may have `value` among the values of the
    /// field `key` (`entry::keys::TAGS`), or `None` when it has nothing to
    /// say for them. Blocking: asked on the search's worker, which can
    /// cancel.
    fn candidates(
        &self,
        key: &str,
        value: &str,
        root: &Path,
        cancel: &CancelToken,
    ) -> Option<Vec<PathBuf>>;
}

/// The registered hint providers, in registration order.
#[derive(Default)]
pub struct Hints {
    list: Contributions<Arc<dyn HintProvider>>,
}

impl Hints {
    pub fn gated(gate: Arc<OwnerGate>) -> Self {
        Hints {
            list: Contributions::gated(gate),
        }
    }

    pub fn register(&self, owner: &str, provider: Arc<dyn HintProvider>) {
        self.list.add(owner, provider);
    }

    pub fn unregister_owner(&self, owner: &str) {
        self.list.remove_owner(owner);
    }

    pub fn ids(&self) -> Vec<&'static str> {
        self.list.all().iter().map(|p| p.id()).collect()
    }

    /// Every provider's candidates, once each, only those under `root`;
    /// `None` when no provider had anything to say.
    pub fn candidates(
        &self,
        key: &str,
        value: &str,
        root: &Path,
        cancel: &CancelToken,
    ) -> Option<Vec<PathBuf>> {
        let providers = self.list.all();
        let mut said = false;
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for p in providers {
            if cancel.is_cancelled() {
                return None;
            }
            let Some(paths) = p.candidates(key, value, root, cancel) else {
                continue;
            };
            said = true;
            for path in paths {
                if path.starts_with(root) && path != root && seen.insert(path.clone()) {
                    out.push(path);
                }
            }
        }
        said.then_some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixed(Option<Vec<&'static str>>);

    impl HintProvider for Fixed {
        fn id(&self) -> &'static str {
            "fixed"
        }
        fn candidates(&self, _: &str, _: &str, _: &Path, _: &CancelToken) -> Option<Vec<PathBuf>> {
            self.0
                .as_ref()
                .map(|paths| paths.iter().map(PathBuf::from).collect())
        }
    }

    #[test]
    fn answers_are_merged_once_each_and_kept_under_the_root() {
        let hints = Hints::default();
        let cancel = CancelToken::new();
        let ask = |hints: &Hints| hints.candidates("tags", "x", Path::new("/r"), &cancel);
        assert_eq!(ask(&hints), None, "nobody to ask");
        hints.register("a", Arc::new(Fixed(None)));
        assert_eq!(ask(&hints), None, "nobody had anything to say");
        hints.register(
            "b",
            Arc::new(Fixed(Some(vec!["/r/1", "/elsewhere/2", "/r"]))),
        );
        hints.register("c", Arc::new(Fixed(Some(vec!["/r/it's 3", "/r/1"]))));
        assert_eq!(
            ask(&hints).unwrap(),
            [PathBuf::from("/r/1"), PathBuf::from("/r/it's 3")]
        );
        hints.unregister_owner("b");
        hints.unregister_owner("c");
        assert_eq!(ask(&hints), None);
        hints.register("d", Arc::new(Fixed(Some(Vec::new()))));
        assert_eq!(ask(&hints), Some(Vec::new()), "an index that knows of none");
    }
}
