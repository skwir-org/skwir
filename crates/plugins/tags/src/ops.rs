//! Operations written before the notes stores (ADR-027): a plan that holds
//! `set_tags`, `set_rating` or `set_comment` (an agent's, a script's) is
//! read as the edit it was, a core `set_notes` written as the stores are
//! set to be now.

use std::sync::Arc;

use skwir_core::app::App;
use skwir_core::op::EntryRef;

#[derive(serde::Deserialize)]
struct Tags {
    entry: EntryRef,
    tags: Vec<String>,
}

#[derive(serde::Deserialize)]
struct Rating {
    entry: EntryRef,
    rating: Option<u64>,
}

#[derive(serde::Deserialize)]
struct Comment {
    entry: EntryRef,
    comment: Option<String>,
}

/// Register the three kinds, each read as a `set_notes`.
pub fn register_legacy(app: &App) {
    let stores = Arc::clone(&app.notes);
    app.ops.register_with(crate::ID, "set_tags", move |v| {
        let a: Tags = serde_json::from_value(v)?;
        Ok(Box::new(stores.edit(a.entry).tags(a.tags)))
    });
    let stores = Arc::clone(&app.notes);
    app.ops.register_with(crate::ID, "set_rating", move |v| {
        let a: Rating = serde_json::from_value(v)?;
        Ok(Box::new(stores.edit(a.entry).rating(a.rating)))
    });
    let stores = Arc::clone(&app.notes);
    app.ops.register_with(crate::ID, "set_comment", move |v| {
        let a: Comment = serde_json::from_value(v)?;
        Ok(Box::new(stores.edit(a.entry).comment(a.comment)))
    });
}
