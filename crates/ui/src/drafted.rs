//! A translation still a draft says so (ADR-139, 13). Automatic speaks a
//! language once it is translated, before a person who speaks it reviewed
//! it: the first start it does, a toast says the translation is a draft,
//! with a button for English. Once a language, kept in the state file
//! (`TOLD_KEY`): it never comes back. About says it for as long as the
//! window speaks it, chosen or not (`about.rs`); the Language setting
//! lists it as a draft, with how much of it is reviewed.
//!
//! The words say a draft, not who made it: a catalog records whether a
//! message is reviewed (its `# draft` mark), not whether a model or a
//! person wrote it.

use std::sync::Arc;

use skwir_core::App;
use skwir_core::app::NotifyLevel;
use skwir_core::command::Invocation;
use skwir_core::words;

/// What the state file keeps: the languages whose draft was told.
pub const TOLD_KEY: &str = "ui.drafts_told";

/// Tells, once a language, that Automatic speaks a translation still a
/// draft: called once, as the application starts, after the window's
/// settings are declared (`TOLD_KEY` among them: an undeclared key is
/// neither read nor saved).
pub fn tell(app: &Arc<App>) {
    let Some(draft) = skwir_core::locale::draft_spoken().filter(|draft| draft.automatic) else {
        return;
    };
    let mut told: Vec<String> = app.settings.get(TOLD_KEY).unwrap_or_default();
    if told.contains(&draft.language) {
        return;
    }
    told.push(draft.language);
    app.settings.set(
        TOLD_KEY,
        toml::Value::Array(told.into_iter().map(toml::Value::String).collect()),
    );
    let weak = app.weak();
    app.scheduler.spawn(
        skwir_core::scheduler::Priority::Background,
        "ui-drafts-told",
        app.lifetime().child(),
        move || {
            if let Some(app) = weak.upgrade()
                && let Err(error) = app.settings.save()
            {
                tracing::warn!("cannot save which drafts were told: {error:#}");
            }
        },
    );
    app.notify_with_action(
        NotifyLevel::Info,
        notice(),
        words!(
            drafts_use_english = "Use English",
            note = "The button of the toast saying the translation is a draft: Skwir speaks English from then on (Settings › Language)."
        ),
        Invocation {
            name: "settings.apply".into(),
            args: serde_json::json!({
                "changes": { skwir_core::locale::LANGUAGE_KEY: skwir_core::locale::ENGLISH }
            }),
        },
    );
}

/// What the toast says, in the language it is about.
fn notice() -> words::Words {
    words!(
        drafts_notice = "This translation is a draft, not fully reviewed yet.",
        note = "Said once, the first time Skwir speaks a translation that a person who speaks the language has not reviewed yet: this one. Say a draft, not who made it."
    )
}

#[cfg(test)]
mod tests {
    use skwir_core::App;
    use skwir_core::words::{Offer, Share, Shipped};

    static SHIPPED: [Shipped; 1] = [Shipped {
        domain: "skwir-core",
        language: "fr",
        source: "",
    }];
    static DRAFTED: [Share; 1] = [Share {
        language: "fr",
        messages: 100,
        translated: 100,
        reviewed: 3,
        offered: Offer::Draft,
    }];

    /// Automatic speaking a draft says so once, with English a click
    /// away; a language the user chose is not told, nor one told before.
    #[test]
    fn a_draft_automatic_speaks_is_told_once() {
        let tmp = tempfile::tempdir().unwrap();
        let app = App::new(skwir_core::app::AppConfig::ephemeral(tmp.path())).unwrap();
        crate::settings::declare(&app);
        skwir_core::words::ship(&SHIPPED);
        skwir_core::words::ship_shares(&DRAFTED);
        skwir_core::locale::set_system(vec!["fr-FR".into()]);
        skwir_core::locale::set_chosen(Some("fr".into()));
        super::tell(&app);
        assert!(app.take_notifications().is_empty(), "chosen, not told");

        skwir_core::locale::set_chosen(None);
        super::tell(&app);
        let told = app.take_notifications();
        assert_eq!(told.len(), 1);
        assert_eq!(told[0].text, super::notice());
        let action = told[0].action.as_ref().expect("a button");
        assert_eq!(action.label.english(), "Use English");
        assert_eq!(&*action.command.name, "settings.apply");
        assert_eq!(action.command.args["changes"]["core.language"], "en");
        let kept: Vec<String> = app.settings.get(super::TOLD_KEY).unwrap();
        assert_eq!(kept, ["fr"]);

        super::tell(&app);
        assert!(app.take_notifications().is_empty(), "told once");
    }
}
