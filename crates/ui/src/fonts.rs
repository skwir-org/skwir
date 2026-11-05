//! The fonts skwir writes in, by role (ADR-073): the interface's; the
//! reading font of what is read (Markdown and HTML, documents, books,
//! mail); the code font of what is read a character at a time (the code
//! view, code in documents, the byte view, the terminal). Each is
//! Automatic or a family the user chose (`ui.font`, `reading.font`,
//! `ui.code_font`). Automatic is the desktop's as Fonts from reads it
//! (`desktop_style::fonts`, ADR-117), else skwir's own.
//!
//! They are tokens of the theme (`Theme::font`, `reading_font`,
//! `code_font`), resolved with the rest of the look (`Theme::resolve`)
//! whenever the settings or the desktop's style move: nothing keeps a
//! copy, so a change shows at once. A family is asked whether it loads
//! (`font_id`), never resolved: resolving one that is missing falls back
//! to gpui's own stack and panics when none of it is installed either (a
//! minimal system with only Liberation or Inter), and a family that does
//! not resolve costs a fallback search on every frame (`AGENTS.md`).
//!
//! Skwir's own, Inter and JetBrains Mono, come with its packages
//! (ADR-118): the files of a family a role needs and the system lacks are
//! read on a thread of their own, ahead of the window at launch
//! (`read_ahead`), and registered as the theme resolves (`carry`); a
//! window in the desktop's fonts reads none.

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

/// The families of `app`'s settings and the desktop's, against `text`,
/// once the fonts skwir's packages carry that a role needs are registered
/// (`carry`).
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
    let desktop = crate::desktop_style::fonts(&app.settings);
    carry(app, text, &needed(&app.settings, &desktop, loads));
    choose(&app.settings, &desktop, loads, any)
}

/// The fonts skwir's packages carry (`skwir_platform::fonts::CARRIED`,
/// ADR-118) that a role would be drawn in and that do not `load`: walking
/// each role's families as `choose` does (the one chosen, the desktop's,
/// skwir's own first), the first carried family met before one that loads.
/// So Inter chosen and not installed is needed, whatever the desktop lends,
/// and skwir's own only where nothing chosen or lent loads: a window in the
/// desktop's fonts needs none, one on macOS or Windows only JetBrains Mono
/// for code. Reading falls back on the interface's family, so it needs one
/// only chosen or lent.
pub(crate) fn needed(
    settings: &Settings,
    desktop: &Desktop,
    loads: impl Fn(&str) -> bool,
) -> Vec<&'static str> {
    let roles = [
        (FONT_KEY, &desktop.interface, Some(INTERFACE[0])),
        (READING_FONT_KEY, &desktop.document, None),
        (CODE_FONT_KEY, &desktop.monospace, Some(CODE[0])),
    ];
    let mut needed = Vec::new();
    for (key, lent, own) in roles {
        let chosen = chosen(settings, key);
        for family in chosen
            .as_deref()
            .into_iter()
            .chain(lent.as_deref())
            .chain(own)
        {
            let carried = skwir_platform::fonts::CARRIED
                .into_iter()
                .find(|carried| *carried == family);
            if let Some(carried) = carried
                && !loads(carried)
            {
                if !needed.contains(&carried) {
                    needed.push(carried);
                }
                break;
            }
            if loads(family) {
                break;
            }
        }
    }
    needed
}

/// What this run has read of the fonts skwir's packages carry.
struct Carried {
    /// Files being read (`read`), not taken yet.
    reading: Option<Reading>,
    /// The families read, registered or not found: never read again.
    read: Vec<&'static str>,
}

/// Files of the carried fonts read on a thread of their own.
struct Reading {
    families: Vec<&'static str>,
    files: std::sync::mpsc::Receiver<Vec<(&'static str, Vec<u8>)>>,
    /// Read ahead at launch (`read_ahead`): the window's first theme
    /// waits for them, so it is drawn in them from its first frame.
    awaited: bool,
}

fn carried() -> std::sync::MutexGuard<'static, Carried> {
    static CARRIED: std::sync::Mutex<Carried> = std::sync::Mutex::new(Carried {
        reading: None,
        read: Vec::new(),
    });
    CARRIED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Moves each time fonts read after launch are in: the theme is resolved
/// again (`Shell::sync`), which registers them.
static READ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// What `Shell::sync` counts among what the theme is made of.
pub fn revision() -> u64 {
    READ.load(std::sync::atomic::Ordering::Acquire)
}

/// Start reading, on a thread of its own, the fonts skwir's packages
/// carry that the settings and what the desktop said when skwir last ran
/// (`desktop_style::start`) will draw in, as `needed` finds them taking a
/// carried family as not installed and any other as installed: nothing
/// says yet what the text system has. Called at launch, before the window
/// opens; its first theme takes them (`carry`).
pub fn read_ahead(app: &App) {
    let desktop = crate::desktop_style::fonts(&app.settings);
    let families = needed(&app.settings, &desktop, |family| {
        !skwir_platform::fonts::CARRIED.contains(&family)
    });
    if !families.is_empty() {
        carried().reading = read(families, true, None);
    }
}

/// Read the carried files of `families` on a thread of its own, then wake
/// with `waker`, if any. Files no longer wanted when read are dropped
/// there, where they were allocated.
fn read(
    families: Vec<&'static str>,
    awaited: bool,
    waker: Option<skwir_core::location::Waker>,
) -> Option<Reading> {
    let (sender, files) = std::sync::mpsc::sync_channel(1);
    let asked = families.clone();
    let spawned = std::thread::Builder::new()
        .name("skwir-fonts".to_owned())
        .spawn(move || {
            let read = skwir_platform::fonts::bundled(&asked);
            if sender.send(read).is_ok()
                && let Some(waker) = waker
            {
                READ.fetch_add(1, std::sync::atomic::Ordering::Release);
                waker();
            }
        });
    match spawned {
        Ok(_) => Some(Reading {
            families,
            files,
            awaited,
        }),
        Err(error) => {
            tracing::warn!(%error, "the fonts skwir carries are not read");
            None
        }
    }
}

/// Register with `text` the fonts skwir's packages carry of the families
/// `needed`, each family once a run and one file at a time (DirectWrite
/// drops a whole batch for one file it refuses). Those read ahead at
/// launch are waited for; a family needed later (another choice, Fonts
/// from on skwir's own) is read on a thread, and the theme resolved again
/// once it is in (`revision`), so the window never waits on the disk.
fn carry(app: &App, text: &gpui::TextSystem, needed: &[&'static str]) {
    let mut carried = carried();
    let wanted: Vec<&'static str> = needed
        .iter()
        .copied()
        .filter(|family| !carried.read.contains(family))
        .collect();
    let mut files = Vec::new();
    if let Some(reading) = carried.reading.take() {
        let taken = if !reading
            .families
            .iter()
            .any(|family| wanted.contains(family))
        {
            // Read for nothing (an Inter installed): let go.
            None
        } else if reading.awaited {
            let started = std::time::Instant::now();
            let read = reading.files.recv().unwrap_or_default();
            tracing::debug!(waited = ?started.elapsed(), "the fonts skwir carries, read ahead");
            Some(read)
        } else {
            match reading.files.try_recv() {
                Ok(read) => Some(read),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(Vec::new()),
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    // Still reading: it wakes the window when done.
                    carried.reading = Some(reading);
                    return;
                }
            }
        };
        if let Some(read) = taken {
            let families = reading.families;
            carried.read.extend(
                families
                    .into_iter()
                    .filter(|family| wanted.contains(family)),
            );
            files = read;
        }
    }
    let mut count = 0;
    for (family, bytes) in files {
        if !wanted.contains(&family) {
            continue;
        }
        match text.add_fonts(vec![std::borrow::Cow::Owned(bytes)]) {
            Ok(()) => count += 1,
            Err(error) => tracing::warn!(family, %error, "a font skwir carries did not load"),
        }
    }
    if count > 0 {
        tracing::info!(count, "fonts skwir carries");
    }
    let missing: Vec<&'static str> = wanted
        .into_iter()
        .filter(|family| !carried.read.contains(family))
        .collect();
    if !missing.is_empty() {
        carried.reading = read(missing, false, Some(app.waker_handle()));
    }
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
    choose_with(|key| chosen(settings, key), desktop, loads, any)
}

/// What each role's Automatic comes to, against `text`: what the settings
/// say beside it (`settings::show_resolved`).
pub fn automatic(app: &App, text: &gpui::TextSystem) -> Families {
    let loads = |family: &str| text.font_id(&gpui::font(family.to_owned())).is_ok();
    let any = || {
        text.all_font_names()
            .into_iter()
            .find(|family| !family.starts_with('.') && loads(family))
    };
    choose_with(
        |_| None,
        &crate::desktop_style::fonts(&app.settings),
        loads,
        any,
    )
}

/// `choose`, a role's family chosen as `chosen` says.
fn choose_with(
    chosen: impl Fn(&str) -> Option<String>,
    desktop: &Desktop,
    loads: impl Fn(&str) -> bool,
    any: impl FnOnce() -> Option<String>,
) -> Families {
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

/// A family as a person knows it: one the system keeps apart from those
/// listed (a leading `.`, macOS's `.AppleSystemUIFont`) is the system font.
pub fn named(family: &str) -> &str {
    if family.starts_with('.') {
        "the system font"
    } else {
        family
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
    fn skwir_s_own_first_families_are_those_its_packages_carry() {
        for family in [INTERFACE[0], CODE[0]] {
            assert!(skwir_platform::fonts::CARRIED.contains(&family), "{family}");
        }
    }

    #[test]
    fn a_carried_font_is_needed_only_by_a_role_that_reaches_it() {
        let (_tmp, app) = app();
        let lent = desktop();
        let desktop_s = system(&["Noto Sans", "Noto Serif", "Hack"]);
        // In the desktop's fonts: none.
        assert!(needed(&app.settings, &lent, &desktop_s).is_empty());
        // macOS and Windows lend the interface's alone: code's.
        let interface_only = Desktop {
            interface: Some("Noto Sans".into()),
            ..Desktop::default()
        };
        assert_eq!(
            needed(&app.settings, &interface_only, &desktop_s),
            ["JetBrains Mono"]
        );
        // Nothing lent: both, but one installed.
        assert_eq!(
            needed(&app.settings, &Desktop::default(), &desktop_s),
            ["Inter", "JetBrains Mono"]
        );
        assert_eq!(
            needed(
                &app.settings,
                &Desktop::default(),
                system(&["JetBrains Mono"])
            ),
            ["Inter"]
        );
        // A chosen family that does not load reaches the desktop's.
        set(&app, FONT_KEY, "Missing Sans");
        assert!(needed(&app.settings, &lent, &desktop_s).is_empty());
        // Inter chosen and not installed: needed whatever is lent, for
        // reading too.
        set(&app, FONT_KEY, "Inter");
        assert_eq!(needed(&app.settings, &lent, &desktop_s), ["Inter"]);
        set(&app, FONT_KEY, FONT_AUTO);
        set(&app, READING_FONT_KEY, "Inter");
        assert_eq!(needed(&app.settings, &lent, &desktop_s), ["Inter"]);
        // Installed, it is the system's.
        assert!(
            needed(
                &app.settings,
                &lent,
                system(&["Noto Sans", "Hack", "Inter"])
            )
            .is_empty()
        );
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
        // A family the desktop names that does not load is passed over,
        // for the system's own.
        let families = ["Noto Sans", CODE[1]];
        let some = system(&families);
        let chosen = choose(&app.settings, &desktop(), some, none);
        assert_eq!(names(&chosen), ["Noto Sans", "Noto Sans", CODE[1]]);
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
