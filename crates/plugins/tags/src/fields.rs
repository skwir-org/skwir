//! Categories that hold one tag at a time: fields of the file. `Status`
//! marked so makes `Status/Draft`, `Status/Review` and `Status/Final` the
//! values of a field `Status`: a file given one loses the other
//! (`metadata::with_exclusive`), the tag editor offers them as a choice,
//! and Details shows the field with its value. A field is still a
//! category of tags, kept on the file as any tag is; only the mark is
//! bibli's (`metadata::TAG_EXCLUSIVE_SETTING`), and it follows the
//! category renamed (`follow.rs`).

use std::sync::Arc;

use bibli_core::app::{App, NotifyLevel};
use bibli_core::command::Invocation;
use bibli_core::metadata::{TAG_EXCLUSIVE_SETTING, clean_tag, same_tag};
use parking_lot::Mutex;

/// The categories marked, each once.
pub fn exclusive(app: &App) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for tag in app
        .settings
        .get::<Vec<String>>(TAG_EXCLUSIVE_SETTING)
        .unwrap_or_default()
    {
        let tag = clean_tag(&tag);
        if !tag.is_empty() && !out.iter().any(|t| same_tag(t, &tag)) {
            out.push(tag);
        }
    }
    out
}

/// The categories marked with `tag` marked (`on`) or not.
pub fn marked(list: &[String], tag: &str, on: bool) -> Vec<String> {
    let mut out: Vec<String> = list.iter().filter(|t| !same_tag(t, tag)).cloned().collect();
    if on {
        out.push(tag.to_owned());
    }
    out
}

/// Mark the category `tag` as holding one tag at a time, or not, and save,
/// on a worker; a toast says so, with Undo (Redo once `undo`).
pub fn set(app: &App, saving: &Arc<Mutex<()>>, tag: &str, on: bool, undo: bool) {
    let tag = clean_tag(tag);
    let (saving, weak) = (Arc::clone(saving), app.weak());
    app.scheduler.spawn(
        bibli_core::scheduler::Priority::Normal,
        "tags.exclusive",
        app.lifetime().child(),
        move || {
            let Some(app) = weak.upgrade() else { return };
            let _saving = saving.lock();
            let was = exclusive(&app);
            let list = marked(&was, &tag, on);
            if list.len() == was.len() && list.iter().all(|t| was.iter().any(|w| same_tag(w, t))) {
                return;
            }
            app.settings.set(
                TAG_EXCLUSIVE_SETTING,
                toml::Value::Array(list.into_iter().map(toml::Value::String).collect()),
            );
            match app.settings.save() {
                Err(error) => app.notify(
                    NotifyLevel::Error,
                    format!("“{tag}” was not changed: {error:#}"),
                ),
                Ok(()) => app.notify_with_action(
                    NotifyLevel::Info,
                    if on {
                        format!("A file now has one tag of “{tag}” at a time")
                    } else {
                        format!("A file can have several tags of “{tag}” again")
                    },
                    if undo { "Redo" } else { "Undo" },
                    Invocation::with_args(
                        "tags.exclusive",
                        &serde_json::json!({ "tag": tag, "on": !on, "undo": !undo }),
                    ),
                ),
            }
            app.wake();
        },
    );
}

#[cfg(test)]
mod tests {
    use super::marked;

    #[test]
    fn a_category_is_marked_once() {
        let list = vec!["Status".to_owned()];
        assert_eq!(marked(&list, "status", true), ["status"]);
        assert_eq!(marked(&list, "Priority", true), ["Status", "Priority"]);
        assert!(marked(&list, "STATUS", false).is_empty());
    }
}
