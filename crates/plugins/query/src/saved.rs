//! Saved searches and saved places: the list the sidebar's Saved section
//! shows and the edits that change it.
//!
//! Pure. Applying an edit returns the edit that reverts it, which the
//! notification offers as Undo (and the inverse of that as Redo). A saved
//! search is known by its expression, as a bookmark is by its folder: two
//! entries with the same one would open the same results. A saved place
//! (ADR-109) is a location kept with what filters it there: known by that
//! filter, its `expr`, and where it is, `at`. Their [`Key`] is what every
//! edit names and what `before` points at.

use skwir_core::location::Location;
use skwir_core::os_path::PathArg;

/// One saved search or saved place, as stored under `query.saved`.
#[derive(
    Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema, PartialEq, Eq,
)]
pub struct SavedQuery {
    pub name: String,
    /// The search; for a saved place, what filters it (the `filter`
    /// command's words).
    pub expr: String,
    /// Where a saved place is, shown filtered by `expr` alone (ADR-109);
    /// `None` for a search. A location, its exact path kept whatever
    /// bytes it holds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<Location>,
}

impl SavedQuery {
    /// What names it among the others.
    pub fn key(&self) -> Key {
        Key {
            expr: self.expr.clone(),
            at: self.at.clone(),
        }
    }
}

/// A saved search by its expression, or a saved place by its filter and
/// where it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Key {
    pub expr: String,
    pub at: Option<Location>,
}

impl From<&str> for Key {
    /// A saved search's.
    fn from(expr: &str) -> Self {
        Key {
            expr: expr.to_owned(),
            at: None,
        }
    }
}

impl Key {
    /// As a command's argument names one ([`KeyArg`]): a search by its
    /// expression, a place as `{"expr": …, "at": …}`.
    pub fn json(&self) -> serde_json::Value {
        match &self.at {
            None => serde_json::json!(self.expr),
            Some(at) => serde_json::json!({ "expr": self.expr, "at": at.arg() }),
        }
    }
}

/// A saved entry named in a command's arguments (`before`): a search's
/// expression, or `{"expr": …, "at": …}` for a saved place.
#[derive(Clone, Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum KeyArg {
    Kept {
        expr: String,
        #[serde(default)]
        at: Option<PathArg>,
    },
    Search(String),
}

impl From<KeyArg> for Key {
    fn from(arg: KeyArg) -> Self {
        match arg {
            KeyArg::Kept { expr, at } => Key {
                expr,
                at: at.as_ref().map(at_location),
            },
            KeyArg::Search(expr) => Key { expr, at: None },
        }
    }
}

/// A saved place's `at` as a command's argument gives it: a path or
/// `scheme:rest`, as `navigate` reads its `location`.
pub fn at_location(at: &PathArg) -> Location {
    Location::parse_os(at.as_os_str())
}

/// A change to the saved list. `before` names the entry this one goes in
/// front of; `None` (or one no longer saved) puts it last.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Edit {
    Add {
        name: String,
        key: Key,
        before: Option<Key>,
    },
    Remove {
        key: Key,
    },
    Rename {
        key: Key,
        name: String,
    },
    Move {
        key: Key,
        before: Option<Key>,
    },
    /// The search, or a place's filter, changes; its name and place stay.
    Change {
        key: Key,
        to: String,
    },
}

impl Edit {
    /// The entry the edit is about, as it is before the edit.
    pub fn key(&self) -> &Key {
        match self {
            Edit::Add { key, .. }
            | Edit::Remove { key }
            | Edit::Rename { key, .. }
            | Edit::Move { key, .. }
            | Edit::Change { key, .. } => key,
        }
    }

    /// Apply the edit to `list`: the edit that reverts it when something
    /// changed, `None` when there was nothing to do (already saved, not
    /// saved, the same name or place). An error leaves `list` as it was.
    pub fn apply(&self, list: &mut Vec<SavedQuery>) -> Result<Option<Edit>, String> {
        let find = |list: &[SavedQuery], key: &Key| list.iter().position(|s| s.key() == *key);
        // What follows the entry at `i`: where it goes back to.
        let next = |list: &[SavedQuery], i: usize| list.get(i + 1).map(SavedQuery::key);
        let insert = |list: &mut Vec<SavedQuery>, entry, before: &Option<Key>| {
            let at = before
                .as_ref()
                .and_then(|b| find(list, b))
                .unwrap_or(list.len());
            list.insert(at, entry);
        };
        Ok(match self {
            Edit::Add { name, key, before } => {
                if find(list, key).is_some() {
                    return Ok(None);
                }
                let entry = SavedQuery {
                    name: name.clone(),
                    expr: key.expr.clone(),
                    at: key.at.clone(),
                };
                insert(list, entry, before);
                Some(Edit::Remove { key: key.clone() })
            }
            Edit::Remove { key } => {
                let Some(i) = find(list, key) else {
                    return Ok(None);
                };
                let before = next(list, i);
                let removed = list.remove(i);
                Some(Edit::Add {
                    key: removed.key(),
                    name: removed.name,
                    before,
                })
            }
            Edit::Rename { key, name } => {
                let Some(entry) = list.iter_mut().find(|s| s.key() == *key) else {
                    return Ok(None);
                };
                if entry.name == *name {
                    return Ok(None);
                }
                let old = std::mem::replace(&mut entry.name, name.clone());
                Some(Edit::Rename {
                    key: key.clone(),
                    name: old,
                })
            }
            Edit::Move { key, before } => {
                let Some(i) = find(list, key) else {
                    return Ok(None);
                };
                let back = next(list, i);
                let original = list.clone();
                let entry = list.remove(i);
                insert(list, entry, before);
                if *list == original {
                    return Ok(None);
                }
                Some(Edit::Move {
                    key: key.clone(),
                    before: back,
                })
            }
            Edit::Change { key, to } => {
                if key.expr == *to {
                    return Ok(None);
                }
                let changed = Key {
                    expr: to.clone(),
                    at: key.at.clone(),
                };
                if let Some(other) = list.iter().find(|s| s.key() == changed) {
                    return Err(match &key.at {
                        None => format!("this search is already saved as “{}”", other.name),
                        Some(_) => format!("this filter is already saved as “{}”", other.name),
                    });
                }
                let Some(entry) = list.iter_mut().find(|s| s.key() == *key) else {
                    return Ok(None);
                };
                entry.expr.clone_from(to);
                Some(Edit::Change {
                    key: changed,
                    to: key.expr.clone(),
                })
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(exprs: &[&str]) -> Vec<SavedQuery> {
        exprs
            .iter()
            .map(|e| SavedQuery {
                name: format!("n-{e}"),
                expr: (*e).to_owned(),
                at: None,
            })
            .collect()
    }

    fn exprs(list: &[SavedQuery]) -> Vec<&str> {
        list.iter().map(|s| s.expr.as_str()).collect()
    }

    /// Apply `edit`, then its inverse, then that one's inverse: undo gives
    /// back the list before, redo the list after.
    fn round_trip(start: &[&str], edit: &Edit) -> Vec<SavedQuery> {
        let before = list(start);
        let mut after = before.clone();
        let inverse = edit
            .apply(&mut after)
            .unwrap()
            .expect("it changes the list");
        let mut undone = after.clone();
        let redo = inverse
            .apply(&mut undone)
            .unwrap()
            .expect("undo changes it");
        assert_eq!(undone, before, "undo of {edit:?}");
        let mut redone = undone;
        redo.apply(&mut redone).unwrap().expect("redo changes it");
        assert_eq!(redone, after, "redo of {edit:?}");
        after
    }

    #[test]
    fn every_edit_is_undone_and_redone() {
        let add = |expr: &str, before: Option<&str>| Edit::Add {
            name: "new".into(),
            key: expr.into(),
            before: before.map(Into::into),
        };
        assert_eq!(
            exprs(&round_trip(&["a", "b"], &add("c", None))),
            ["a", "b", "c"]
        );
        assert_eq!(
            exprs(&round_trip(&["a", "b"], &add("c", Some("b")))),
            ["a", "c", "b"]
        );
        let remove = |expr: &str| Edit::Remove { key: expr.into() };
        // Undoing a removal puts it back where it was, first, middle or last.
        for gone in ["a", "b", "c"] {
            round_trip(&["a", "b", "c"], &remove(gone));
        }
        let after = round_trip(
            &["a", "b"],
            &Edit::Rename {
                key: "a".into(),
                name: "Renamed".into(),
            },
        );
        assert_eq!(after[0].name, "Renamed");
        let mv = |expr: &str, before: Option<&str>| Edit::Move {
            key: expr.into(),
            before: before.map(Into::into),
        };
        assert_eq!(
            exprs(&round_trip(&["a", "b", "c"], &mv("a", None))),
            ["b", "c", "a"]
        );
        assert_eq!(
            exprs(&round_trip(&["a", "b", "c"], &mv("c", Some("a")))),
            ["c", "a", "b"]
        );
        assert_eq!(
            exprs(&round_trip(&["a", "b", "c"], &mv("a", Some("c")))),
            ["b", "a", "c"]
        );
        let after = round_trip(
            &["a", "b", "c"],
            &Edit::Change {
                key: "b".into(),
                to: "z".into(),
            },
        );
        assert_eq!(exprs(&after), ["a", "z", "c"], "keeps its place");
        assert_eq!(after[1].name, "n-b", "and its name");
    }

    #[test]
    fn an_edit_with_nothing_to_do_says_so_and_changes_nothing() {
        let start = list(&["a", "b"]);
        for edit in [
            Edit::Add {
                name: "again".into(),
                key: "a".into(),
                before: None,
            },
            Edit::Remove { key: "x".into() },
            Edit::Rename {
                key: "a".into(),
                name: "n-a".into(),
            },
            Edit::Rename {
                key: "x".into(),
                name: "y".into(),
            },
            Edit::Move {
                key: "a".into(),
                before: Some("b".into()),
            },
            Edit::Move {
                key: "b".into(),
                before: None,
            },
            Edit::Change {
                key: "a".into(),
                to: "a".into(),
            },
        ] {
            let mut l = start.clone();
            assert_eq!(edit.apply(&mut l), Ok(None), "{edit:?}");
            assert_eq!(l, start, "{edit:?}");
        }
    }

    #[test]
    fn a_search_cannot_become_one_already_saved() {
        let start = list(&["a", "b"]);
        let mut l = start.clone();
        let edit = Edit::Change {
            key: "a".into(),
            to: "b".into(),
        };
        assert!(edit.apply(&mut l).unwrap_err().contains("n-b"));
        assert_eq!(l, start);
    }
}
