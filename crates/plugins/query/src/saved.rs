//! Saved searches: the list the sidebar shows and the edits that change it.
//!
//! Pure. Applying an edit returns the edit that reverts it, which the
//! notification offers as Undo (and the inverse of that as Redo). A saved
//! search is known by its expression, as a bookmark is by its folder: two
//! entries with the same one would open the same results, so the
//! expression is what every edit names and what `before` points at.

/// One saved search, as stored under `query.saved`.
#[derive(
    Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema, PartialEq, Eq,
)]
pub struct SavedQuery {
    pub name: String,
    pub expr: String,
}

/// A change to the saved searches. `before` names the search the entry goes
/// in front of; `None` (or one no longer saved) puts it last.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Edit {
    Add {
        name: String,
        expr: String,
        before: Option<String>,
    },
    Remove {
        expr: String,
    },
    Rename {
        expr: String,
        name: String,
    },
    Move {
        expr: String,
        before: Option<String>,
    },
    /// The search itself changes; its name and place stay.
    Change {
        expr: String,
        to: String,
    },
}

impl Edit {
    /// The search the edit is about, as it is before the edit.
    pub fn expr(&self) -> &str {
        match self {
            Edit::Add { expr, .. }
            | Edit::Remove { expr }
            | Edit::Rename { expr, .. }
            | Edit::Move { expr, .. }
            | Edit::Change { expr, .. } => expr,
        }
    }

    /// Apply the edit to `list`: the edit that reverts it when something
    /// changed, `None` when there was nothing to do (already saved, not
    /// saved, the same name or place). An error leaves `list` as it was.
    pub fn apply(&self, list: &mut Vec<SavedQuery>) -> Result<Option<Edit>, String> {
        let find = |list: &[SavedQuery], expr: &str| list.iter().position(|s| s.expr == expr);
        // What follows the entry at `i`: where it goes back to.
        let next = |list: &[SavedQuery], i: usize| list.get(i + 1).map(|s| s.expr.clone());
        let insert = |list: &mut Vec<SavedQuery>, entry, before: &Option<String>| {
            let at = before
                .as_deref()
                .and_then(|b| find(list, b))
                .unwrap_or(list.len());
            list.insert(at, entry);
        };
        Ok(match self {
            Edit::Add { name, expr, before } => {
                if find(list, expr).is_some() {
                    return Ok(None);
                }
                let entry = SavedQuery {
                    name: name.clone(),
                    expr: expr.clone(),
                };
                insert(list, entry, before);
                Some(Edit::Remove { expr: expr.clone() })
            }
            Edit::Remove { expr } => {
                let Some(i) = find(list, expr) else {
                    return Ok(None);
                };
                let before = next(list, i);
                let removed = list.remove(i);
                Some(Edit::Add {
                    name: removed.name,
                    expr: removed.expr,
                    before,
                })
            }
            Edit::Rename { expr, name } => {
                let Some(entry) = list.iter_mut().find(|s| s.expr == *expr) else {
                    return Ok(None);
                };
                if entry.name == *name {
                    return Ok(None);
                }
                let old = std::mem::replace(&mut entry.name, name.clone());
                Some(Edit::Rename {
                    expr: expr.clone(),
                    name: old,
                })
            }
            Edit::Move { expr, before } => {
                let Some(i) = find(list, expr) else {
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
                    expr: expr.clone(),
                    before: back,
                })
            }
            Edit::Change { expr, to } => {
                if expr == to {
                    return Ok(None);
                }
                if let Some(other) = list.iter().find(|s| s.expr == *to) {
                    return Err(format!("this search is already saved as “{}”", other.name));
                }
                let Some(entry) = list.iter_mut().find(|s| s.expr == *expr) else {
                    return Ok(None);
                };
                entry.expr.clone_from(to);
                Some(Edit::Change {
                    expr: to.clone(),
                    to: expr.clone(),
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
            expr: expr.into(),
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
        let remove = |expr: &str| Edit::Remove { expr: expr.into() };
        // Undoing a removal puts it back where it was, first, middle or last.
        for gone in ["a", "b", "c"] {
            round_trip(&["a", "b", "c"], &remove(gone));
        }
        let after = round_trip(
            &["a", "b"],
            &Edit::Rename {
                expr: "a".into(),
                name: "Renamed".into(),
            },
        );
        assert_eq!(after[0].name, "Renamed");
        let mv = |expr: &str, before: Option<&str>| Edit::Move {
            expr: expr.into(),
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
                expr: "b".into(),
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
                expr: "a".into(),
                before: None,
            },
            Edit::Remove { expr: "x".into() },
            Edit::Rename {
                expr: "a".into(),
                name: "n-a".into(),
            },
            Edit::Rename {
                expr: "x".into(),
                name: "y".into(),
            },
            Edit::Move {
                expr: "a".into(),
                before: Some("b".into()),
            },
            Edit::Move {
                expr: "b".into(),
                before: None,
            },
            Edit::Change {
                expr: "a".into(),
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
            expr: "a".into(),
            to: "b".into(),
        };
        assert!(edit.apply(&mut l).unwrap_err().contains("n-b"));
        assert_eq!(l, start);
    }
}
