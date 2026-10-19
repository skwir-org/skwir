//! The fonts skwir writes in, by role (ADR-073): the interface's; the
//! reading font of what is read (Markdown and HTML, documents, books,
//! mail); the code font of what is read a character at a time (the code
//! view, code in documents, the byte view, the terminal). Each is
//! Automatic or a family the user chose (`ui.font`, `reading.font`,
//! `ui.code_font`). Automatic is the desktop's while Desktop colors is on
//! (`desktop_style::fonts`), else skwir's own.
//!
//! They are tokens of the theme (`Theme::font`, `reading_font`,
//! `code_font`), resolved with the rest of the look (`Theme::resolve`)
//! whenever the settings or the desktop's style move: nothing keeps a
//! copy, so a change shows at once. A family is asked whether it loads
//! (`font_id`), never resolved: resolving one that is missing falls back
//! to gpui's own stack and panics when none of it is installed either (a
//! minimal system with only Liberation or Inter), and a family that does
//! not resolve costs a fallback search on every frame (`AGENTS.md`).

use std::collections::HashSet;
use std::sync::Arc;

use gpui::SharedString;
use skwir_core::App;
use skwir_core::settings::Settings;

use crate::desktop_style::Fonts as Desktop;
use crate::settings::{CODE_FONT_KEY, FONT_AUTO, FONT_KEY, READING_FONT_KEY};

/// The interface families tried after the one chosen and the desktop's,
/// in order. Inter first, which a user may have installed anywhere, then
/// each system's own: macOS's and Windows' compiled, never run
/// (ADR-035).
#[cfg(not(any(target_os = "macos", windows)))]
const INTERFACE: &[&str] = &[
    "Inter",
    "DejaVu Sans",
    "Noto Sans",
    "Liberation Sans",
    "Cantarell",
];
#[cfg(target_os = "macos")]
const INTERFACE: &[&str] = &["Inter", "Helvetica Neue", "Helvetica"];
#[cfg(windows)]
const INTERFACE: &[&str] = &["Inter", "Segoe UI", "Tahoma"];

/// The code families tried after the one chosen and the desktop's.
#[cfg(not(any(target_os = "macos", windows)))]
const CODE: &[&str] = &[
    "JetBrains Mono",
    "DejaVu Sans Mono",
    "Noto Sans Mono",
    "Liberation Mono",
];
#[cfg(target_os = "macos")]
const CODE: &[&str] = &["JetBrains Mono", "SF Mono", "Menlo"];
#[cfg(windows)]
const CODE: &[&str] = &["JetBrains Mono", "Cascadia Mono", "Consolas"];

/// The three families a theme holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Families {
    pub ui: SharedString,
    pub reading: SharedString,
    pub code: SharedString,
}

impl Default for Families {
    /// Skwir's first of each, not asked whether they load: what a theme
    /// holds until it is resolved against the text system.
    fn default() -> Self {
        Families {
            ui: INTERFACE[0].into(),
            reading: INTERFACE[0].into(),
            code: CODE[0].into(),
        }
    }
}

/// The families of `app`'s settings and the desktop's, against `text`.
pub fn resolve(app: &App, text: &gpui::TextSystem) -> Families {
    let loads = |family: &str| text.font_id(&gpui::font(family.to_owned())).is_ok();
    let any = || {
        let any = text
            .all_font_names()
            .into_iter()
            .find(|family| !family.starts_with('.') && loads(family));
        tracing::warn!(?any, "none of skwir's fonts is installed");
        any
    };
    choose(
        &app.settings,
        &crate::desktop_style::fonts(&app.settings),
        loads,
        any,
    )
}

/// Each role's family: the one chosen, else the desktop's, else skwir's
/// candidates, the first that `loads`. Reading falls back on the
/// interface's family; when none of the interface's or the code's
/// candidates loads, `any` family the system has, asked once.
pub(crate) fn choose(
    settings: &Settings,
    desktop: &Desktop,
    loads: impl Fn(&str) -> bool,
    any: impl FnOnce() -> Option<String>,
) -> Families {
    let chosen = |key: &str| chosen(settings, key);
    let first = |chosen: Option<String>, desktop: &Option<String>, own: &[&str]| {
        chosen
            .iter()
            .chain(desktop)
            .map(String::as_str)
            .chain(own.iter().copied())
            .find(|family| loads(family))
            .map(str::to_owned)
    };
    let (mut any, mut found) = (Some(any), None);
    let mut any = || {
        found
            .get_or_insert_with(|| any.take().and_then(|any| any()))
            .clone()
    };
    let ui = first(chosen(FONT_KEY), &desktop.interface, INTERFACE)
        .or_else(&mut any)
        .unwrap_or_else(|| INTERFACE[0].to_owned());
    let reading =
        first(chosen(READING_FONT_KEY), &desktop.document, &[]).unwrap_or_else(|| ui.clone());
    let code = first(chosen(CODE_FONT_KEY), &desktop.monospace, CODE)
        .or_else(&mut any)
        .unwrap_or_else(|| CODE[0].to_owned());
    Families {
        ui: ui.into(),
        reading: reading.into(),
        code: code.into(),
    }
}

/// The family `key` names, unless it says Automatic.
pub(crate) fn chosen(settings: &Settings, key: &str) -> Option<String> {
    settings
        .get::<String>(key)
        .filter(|family| !family.is_empty() && family != FONT_AUTO)
}

/// The families the text system can write in, for the font settings'
/// choices (`settings::list_fonts`), and those the system says are
/// monospaced.
#[derive(Clone, Debug, Default)]
pub struct Installed {
    /// Sorted, without the families the system hides (a leading `.`) nor,
    /// where the system says which, those that write no Latin: gpui writes
    /// in none of them.
    pub families: Vec<String>,
    /// `None` where the system does not say (`skwir_platform::fonts`).
    pub monospaced: Option<HashSet<String>>,
}

impl Installed {
    /// What `text` draws and the system says, on a worker: listing the
    /// families reads the text system's index, and what the system says
    /// runs a program.
    pub fn read(text: &Arc<gpui::TextSystem>) -> Self {
        let said = skwir_platform::fonts::families();
        let families = text
            .all_font_names()
            .into_iter()
            .filter(|family| !family.starts_with('.'))
            .filter(|family| said.as_ref().is_none_or(|said| said.latin.contains(family)))
            .collect();
        Installed {
            families,
            monospaced: said.map(|said| said.monospaced),
        }
    }

    /// The families offered for `key`: every one, but for code only the
    /// monospaced ones where the system says which.
    pub fn offered(&self, key: &str) -> Vec<&str> {
        self.families
            .iter()
            .filter(|family| {
                key != CODE_FONT_KEY
                    || self
                        .monospaced
                        .as_ref()
                        .is_none_or(|mono| mono.contains(*family))
            })
            .map(String::as_str)
            .collect()
    }

    pub fn has(&self, family: &str) -> bool {
        self.families.iter().any(|f| f == family)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn app() -> (tempfile::TempDir, Arc<App>) {
        let tmp = tempfile::tempdir().unwrap();
        let app = App::new(skwir_core::app::AppConfig::ephemeral(tmp.path())).unwrap();
        crate::settings::declare(&app);
        (tmp, app)
    }

    fn set(app: &App, key: &str, family: &str) {
        app.settings
            .set(key, toml::Value::String(family.to_owned()));
    }

    /// A system with these families.
    fn system(families: &[&'static str]) -> impl Fn(&str) -> bool {
        let families = families.to_vec();
        move |family| families.contains(&family)
    }

    fn desktop() -> Desktop {
        Desktop {
            interface: Some("Noto Sans".into()),
            document: Some("Noto Serif".into()),
            monospace: Some("Hack".into()),
        }
    }

    fn names(families: &Families) -> [&str; 3] {
        [&families.ui, &families.reading, &families.code]
    }

    #[test]
    fn automatic_is_the_desktop_s_then_skwir_s_own() {
        let (_tmp, app) = app();
        let all = system(&["Noto Sans", "Noto Serif", "Hack", "Inter", "JetBrains Mono"]);
        let none = || -> Option<String> { panic!("a candidate loads") };
        let chosen = choose(&app.settings, &desktop(), &all, none);
        assert_eq!(names(&chosen), ["Noto Sans", "Noto Serif", "Hack"]);
        // The desktop names none: skwir's own, and reading in the
        // interface's.
        let own = choose(&app.settings, &Desktop::default(), &all, none);
        assert_eq!(names(&own), ["Inter", "Inter", "JetBrains Mono"]);
        // A family the desktop names that does not load is passed over.
        let some = system(&["Noto Sans", "DejaVu Sans Mono"]);
        let chosen = choose(&app.settings, &desktop(), some, none);
        assert_eq!(
            names(&chosen),
            ["Noto Sans", "Noto Sans", "DejaVu Sans Mono"]
        );
    }

    #[test]
    fn a_family_chosen_comes_first_and_one_not_installed_is_automatic() {
        let (_tmp, app) = app();
        set(&app, FONT_KEY, "Fira Sans");
        set(&app, READING_FONT_KEY, "Atkinson Hyperlegible");
        set(&app, CODE_FONT_KEY, "Iosevka");
        let all = system(&[
            "Fira Sans",
            "Atkinson Hyperlegible",
            "Iosevka",
            "Noto Sans",
            "Noto Serif",
            "Hack",
        ]);
        let none = || -> Option<String> { panic!("a candidate loads") };
        let chosen = choose(&app.settings, &desktop(), &all, none);
        assert_eq!(
            names(&chosen),
            ["Fira Sans", "Atkinson Hyperlegible", "Iosevka"]
        );
        // Uninstalled since: as if Automatic.
        let without = system(&["Noto Sans", "Noto Serif", "Hack"]);
        let chosen = choose(&app.settings, &desktop(), without, none);
        assert_eq!(names(&chosen), ["Noto Sans", "Noto Serif", "Hack"]);
        // Automatic said in words is Automatic.
        set(&app, FONT_KEY, FONT_AUTO);
        assert_eq!(chosen_family(&app, FONT_KEY), None);
    }

    fn chosen_family(app: &App, key: &str) -> Option<String> {
        chosen(&app.settings, key)
    }

    #[test]
    fn with_none_of_skwir_s_installed_any_family_is_taken_once() {
        let (_tmp, app) = app();
        let asked = std::cell::Cell::new(0);
        let only = system(&["Comic Neue"]);
        let chosen = choose(&app.settings, &Desktop::default(), only, || {
            asked.set(asked.get() + 1);
            Some("Comic Neue".to_owned())
        });
        assert_eq!(names(&chosen), ["Comic Neue", "Comic Neue", "Comic Neue"]);
        assert_eq!(asked.get(), 1, "the system's families are listed once");
    }

    #[test]
    fn code_is_offered_the_monospaced_families_where_the_system_says() {
        let installed = Installed {
            families: vec!["DejaVu Sans".into(), "Hack".into(), "Noto Serif".into()],
            monospaced: Some(["Hack".to_owned()].into()),
        };
        assert_eq!(installed.offered(CODE_FONT_KEY), ["Hack"]);
        assert_eq!(installed.offered(FONT_KEY).len(), 3);
        assert_eq!(installed.offered(READING_FONT_KEY).len(), 3);
        let unsaid = Installed {
            monospaced: None,
            ..installed
        };
        assert_eq!(unsaid.offered(CODE_FONT_KEY).len(), 3);
    }
}
