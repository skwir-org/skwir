//! The operations: set an entry's tags, rating or comment. Each reads what
//! it replaces when applied and returns the operation that puts it back, and
//! says what it does in the user's words for the review and the toast.

use std::path::Path;

use bibli_core::entry::keys;
use bibli_core::op::{Change, EntryRef, Operation, ResolveError, StepContext, StepStatus, Words};

use crate::attrs;

fn name_of(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// The step's status once `entry` resolves: done when `holds` says what it
/// would write is there.
fn check(entry: &EntryRef, holds: impl FnOnce(&Path) -> bool) -> StepStatus {
    match entry.resolve() {
        Ok(p) if holds(&p) => StepStatus::AlreadyDone,
        Ok(_) => StepStatus::Pending { reversible: true },
        Err(ResolveError::Gone(p)) => StepStatus::Conflict {
            reason: format!("{} no longer exists", p.display()),
        },
        Err(e) => StepStatus::Conflict {
            reason: e.to_string(),
        },
    }
}

fn metadata(entry: &EntryRef, field: &str, will: String, did: String) -> Change {
    Change::Metadata {
        path: entry.path.clone(),
        field: field.into(),
        words: Some(Words { will, did }),
    }
}

fn same_set(a: &[String], b: &[String]) -> bool {
    a.len() == b.len()
        && a.iter()
            .all(|x| b.iter().any(|y| bibli_core::metadata::same_tag(x, y)))
}

/// Set the full tag list of an entry.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct SetTags {
    pub entry: EntryRef,
    pub tags: Vec<String>,
}

impl Operation for SetTags {
    fn kind(&self) -> &'static str {
        "set_tags"
    }
    fn clone_box(&self) -> Box<dyn Operation> {
        Box::new(self.clone())
    }
    fn args(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("serializable")
    }
    fn describe(&self) -> Change {
        let name = name_of(&self.entry.path);
        let (will, did) = if self.tags.is_empty() {
            (
                format!("remove the tags of “{name}”"),
                format!("removed the tags of “{name}”"),
            )
        } else {
            let tags = self.tags.join(", ");
            (
                format!("tag “{name}” {tags}"),
                format!("tagged “{name}” {tags}"),
            )
        };
        metadata(&self.entry, keys::TAGS, will, did)
    }
    fn check(&self, _ctx: &StepContext) -> StepStatus {
        check(&self.entry, |p| same_set(&attrs::read_tags(p), &self.tags))
    }
    fn apply(&self, _ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        let path = self.entry.resolve()?;
        let previous = attrs::read_tags(&path);
        attrs::write_tags(&path, &self.tags)
            .map_err(|e| anyhow::anyhow!("set tags on {}: {e}", path.display()))?;
        Ok(Some(Box::new(SetTags {
            entry: EntryRef::new(self.entry.id, path),
            tags: previous,
        })))
    }
}

/// A rating in stars, in words: "4 stars", "3½ stars".
pub fn stars(rating: u64) -> String {
    let whole = rating / 2;
    let half = if rating % 2 == 1 { "½" } else { "" };
    let plural = if rating <= 2 { "" } else { "s" };
    format!("{whole}{half} star{plural}").replacen("0½", "½", 1)
}

/// Set an entry's rating, from 1 to 10 (half stars); `None` removes it.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct SetRating {
    pub entry: EntryRef,
    pub rating: Option<u64>,
}

impl Operation for SetRating {
    fn kind(&self) -> &'static str {
        "set_rating"
    }
    fn clone_box(&self) -> Box<dyn Operation> {
        Box::new(self.clone())
    }
    fn args(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("serializable")
    }
    fn describe(&self) -> Change {
        let name = name_of(&self.entry.path);
        let (will, did) = match self.rating {
            Some(r) => (
                format!("rate “{name}” {}", stars(r)),
                format!("rated “{name}” {}", stars(r)),
            ),
            None => (
                format!("remove the rating of “{name}”"),
                format!("removed the rating of “{name}”"),
            ),
        };
        metadata(&self.entry, keys::RATING, will, did)
    }
    fn check(&self, _ctx: &StepContext) -> StepStatus {
        if self.rating.is_some_and(|r| r == 0 || r > attrs::RATING_MAX) {
            return StepStatus::Conflict {
                reason: format!("a rating is from 1 to {}", attrs::RATING_MAX),
            };
        }
        check(&self.entry, |p| attrs::read_rating(p) == self.rating)
    }
    fn apply(&self, _ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        let path = self.entry.resolve()?;
        let previous = attrs::read_rating(&path);
        let text = self.rating.map(|r| r.to_string());
        attrs::write(&path, attrs::RATING, text.as_deref())
            .map_err(|e| anyhow::anyhow!("rate {}: {e}", path.display()))?;
        Ok(Some(Box::new(SetRating {
            entry: EntryRef::new(self.entry.id, path),
            rating: previous,
        })))
    }
}

/// Set an entry's comment; `None` removes it.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct SetComment {
    pub entry: EntryRef,
    pub comment: Option<String>,
}

impl Operation for SetComment {
    fn kind(&self) -> &'static str {
        "set_comment"
    }
    fn clone_box(&self) -> Box<dyn Operation> {
        Box::new(self.clone())
    }
    fn args(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("serializable")
    }
    fn describe(&self) -> Change {
        let name = name_of(&self.entry.path);
        let (will, did) = match self.comment {
            Some(_) => (
                format!("set the comment of “{name}”"),
                format!("set the comment of “{name}”"),
            ),
            None => (
                format!("remove the comment of “{name}”"),
                format!("removed the comment of “{name}”"),
            ),
        };
        metadata(&self.entry, keys::COMMENT, will, did)
    }
    fn check(&self, _ctx: &StepContext) -> StepStatus {
        check(&self.entry, |p| {
            attrs::read_comment(p).as_deref() == self.comment.as_deref()
        })
    }
    fn apply(&self, _ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        let path = self.entry.resolve()?;
        let previous = attrs::read_comment(&path);
        attrs::write(&path, attrs::COMMENT, self.comment.as_deref())
            .map_err(|e| anyhow::anyhow!("comment on {}: {e}", path.display()))?;
        Ok(Some(Box::new(SetComment {
            entry: EntryRef::new(self.entry.id, path),
            comment: previous,
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rating_reads_as_stars() {
        assert_eq!(stars(10), "5 stars");
        assert_eq!(stars(2), "1 star");
        assert_eq!(stars(7), "3½ stars");
        assert_eq!(stars(1), "½ star");
    }

    #[test]
    fn a_tag_set_ignores_order_and_case() {
        let set = |t: &[&str]| t.iter().map(|&s| s.to_owned()).collect::<Vec<_>>();
        assert!(same_set(&set(&["a", "B"]), &set(&["b", "A"])));
        assert!(!same_set(&set(&["a"]), &set(&["a", "b"])));
    }
}
