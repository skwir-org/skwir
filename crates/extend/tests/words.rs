//! What a plugin from outside says on protocol 2 (ADR-139, 14): its
//! catalogs beside its manifest, registered while it is on and withdrawn
//! when it is turned off; said in the window's language, and in English to
//! agents; what a translation lacks said in English, what its English lacks
//! as its id; a catalog past its bounds left out. A plugin on protocol 1 is
//! shown as it writes. Nothing here runs a plugin: what its manifest says is
//! said without it.

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use skwir_core::App;
use skwir_core::app::AppConfig;

/// The template's manifest, its module one that is never run.
fn template() -> String {
    std::fs::read_to_string(
        common::root()
            .join("crates/plugins-outside/template")
            .join("plugin.toml"),
    )
    .unwrap()
    .replace("module = \"word_count.wasm\"", "module = \"x.wasm\"")
}

/// The template's English, as its author wrote it.
fn template_english() -> String {
    std::fs::read_to_string(common::root().join("crates/plugins-outside/template/l10n/en.ftl"))
        .unwrap()
}

/// A French of this test's own, never the template's shipped draft (its
/// reviewers change that): everything but `count-description`.
const FRENCH: &str = "plugin-title = Compte de mots (essai)\n\
    plugin-description = Combien de mots (essai)\n\
    words-label = Mots (essai)\n\
    count-title = Compter (essai)\n\
    copy-title = Copier (essai)\n\
    copy-description = Pour le presse-papiers (essai)\n\
    count-said = { $count ->\n    [one] { $count } mot (essai)\n   *[other] { $count } mots (essai)\n}\n\
    no-command = pas de { $command } (essai)\n";

/// A plugin folder in `plugins`: `manifest`, a module never run, and its
/// catalogs, each a language and its text.
fn plugin(plugins: &Path, id: &str, manifest: &str, catalogs: &[(&str, &[u8])]) -> PathBuf {
    let folder = plugins.join(id);
    std::fs::create_dir_all(folder.join("l10n")).unwrap();
    std::fs::write(folder.join("plugin.toml"), manifest).unwrap();
    std::fs::write(folder.join("x.wasm"), b"\0asm\x0d\x00\x01\x00").unwrap();
    for (language, text) in catalogs {
        std::fs::write(folder.join("l10n").join(format!("{language}.ftl")), text).unwrap();
    }
    folder
}

/// An app with the plugins of `plugins` found, added and turned on.
fn app(state: &Path, plugins: &Path) -> Arc<App> {
    let app = App::new(AppConfig::ephemeral(state)).unwrap();
    for plugin in skwir_extend::discover(&[plugins.to_owned()], &[]) {
        let id = skwir_core::plugin::Plugin::id(&*plugin);
        app.plugins.add(plugin, &app);
        assert!(app.plugins.set_enabled(id, true, &app), "{id} turned on");
    }
    app
}

/// What the Plugins page lists of `id`: its title and description.
fn listed(app: &App, id: &str) -> (skwir_core::words::Words, skwir_core::words::Words) {
    let info = app
        .plugins
        .list()
        .into_iter()
        .find(|info| info.id == id)
        .expect("listed");
    (info.title, info.description)
}

#[test]
fn a_plugin_on_protocol_2_speaks_the_window_s_language_and_english_to_agents() {
    let tmp = common::home_tempdir();
    let plugins = tmp.path().join("plugins");
    plugin(
        &plugins,
        "word-count",
        &template(),
        &[
            ("en", template_english().as_bytes()),
            ("fr", FRENCH.as_bytes()),
        ],
    );
    // On protocol 1: as it writes, whatever catalogs lie beside it.
    plugin(
        &plugins,
        "older",
        "id = \"older\"\ntitle = \"Older, as written\"\napi = 1\n[run]\nmodule = \"x.wasm\"\n\
         [[commands]]\nname = \"older.run\"\ntitle = \"Run, as written\"\n",
        &[("fr", b"plugin-title = Ignored\n")],
    );
    // A message its English lacks too: its id.
    plugin(
        &plugins,
        "unsaid",
        "id = \"unsaid\"\ntitle = { id = \"nowhere\" }\napi = 2\n[run]\nmodule = \"x.wasm\"\n",
        &[("en", b"other = Other\n")],
    );
    let app = app(&tmp.path().join("state"), &plugins);
    skwir_core::locale::set_chosen(Some("fr".into()));
    let (title, description) = listed(&app, "word-count");
    assert_eq!(title.say_plain(), "Compte de mots (essai)", "the window's");
    assert_eq!(title.english(), "Word count", "an agent's");
    assert_eq!(description.say_plain(), "Combien de mots (essai)");
    let count = app.commands.get("word-count.count").unwrap();
    assert_eq!(count.title.say_plain(), "Compter (essai)");
    assert_eq!(
        count.description.say_plain(),
        "How many words the files hold, each and in all: the selection, or the entries given",
        "a message French lacks: in English"
    );
    // What agents and MCP read: English.
    let sent = serde_json::to_value(&count).unwrap();
    assert_eq!(sent["title"], "Count words");
    let labels: Vec<(&str, String, String)> = app
        .fields
        .shown_for(std::ffi::OsStr::new("a note.txt"))
        .into_iter()
        .map(|(key, label)| (key, label.say_plain(), label.english()))
        .collect();
    assert_eq!(
        labels,
        [(
            "word-count.words",
            "Mots (essai)".to_owned(),
            "Words".to_owned()
        )]
    );
    let (older, _) = listed(&app, "older");
    assert_eq!(older.say_plain(), "Older, as written");
    assert_eq!(
        app.commands.get("older.run").unwrap().title.say_plain(),
        "Run, as written"
    );
    assert!(skwir_core::words::languages_of("outside:older").is_empty());
    let (unsaid, _) = listed(&app, "unsaid");
    assert_eq!(
        (unsaid.say_plain(), unsaid.english()),
        ("nowhere".into(), "nowhere".into())
    );
    // A language it has no catalog in: English.
    skwir_core::locale::set_chosen(Some("zh-TW".into()));
    assert_eq!(listed(&app, "word-count").0.say_plain(), "Word count");
    // Turned off, its catalogs go with its commands; on again, they come
    // back.
    skwir_core::locale::set_chosen(Some("fr".into()));
    assert_eq!(
        skwir_core::words::languages_of("outside:word-count"),
        ["fr"]
    );
    assert!(app.plugins.set_enabled("word-count", false, &app));
    assert!(skwir_core::words::languages_of("outside:word-count").is_empty());
    assert_eq!(
        count.title.say_plain(),
        "Count words",
        "withdrawn: what it says, in English"
    );
    assert!(app.commands.get("word-count.count").is_none());
    // What describes it, on the Plugins page, still the reader's.
    assert_eq!(title.say_plain(), "Compte de mots (essai)");
    assert_eq!(
        listed(&app, "word-count").1.say_plain(),
        "Combien de mots (essai)"
    );
    assert!(app.plugins.set_enabled("word-count", true, &app));
    assert_eq!(title.say_plain(), "Compte de mots (essai)");
    assert_eq!(count.title.say_plain(), "Compter (essai)");
}

/// A plugin from outside is off until the user turns it on, having read on
/// the Plugins page what it does: there it is described in the reader's
/// language from the catalogs its host read when it found it, none of them
/// registered, while nothing else it says is said from them. Turned on, it
/// speaks as before.
#[test]
fn a_plugin_from_outside_that_is_off_is_described_in_the_reader_s_language() {
    let tmp = common::home_tempdir();
    let plugins = tmp.path().join("plugins");
    plugin(
        &plugins,
        "word-count",
        &template(),
        &[
            ("en", template_english().as_bytes()),
            ("fr", FRENCH.as_bytes()),
        ],
    );
    let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
    for plugin in skwir_extend::discover(&[plugins], &[]) {
        app.plugins.add(plugin, &app);
    }
    skwir_core::locale::set_chosen(Some("fr".into()));
    assert!(!app.plugins.is_enabled("word-count"), "off until turned on");
    let (title, description) = listed(&app, "word-count");
    assert_eq!(title.say_plain(), "Compte de mots (essai)");
    assert_eq!(description.say_plain(), "Combien de mots (essai)");
    assert_eq!(title.english(), "Word count", "an agent's");
    let sent = serde_json::to_value(app.plugins.list()).unwrap();
    assert_eq!(sent[0]["title"], "Word count", "what agents read");
    assert!(
        skwir_core::words::languages_of("outside:word-count").is_empty(),
        "nothing registered"
    );
    assert!(app.commands.get("word-count.count").is_none());
    assert!(app.plugins.set_enabled("word-count", true, &app));
    let count = app.commands.get("word-count.count").unwrap();
    assert_eq!(count.title.say_plain(), "Compter (essai)");
    assert!(app.plugins.set_enabled("word-count", false, &app));
    assert_eq!(
        count.title.say_plain(),
        "Count words",
        "off: its commands' words are not"
    );
    assert_eq!(title.say_plain(), "Compte de mots (essai)");
}

#[test]
fn a_catalog_past_its_bounds_is_left_out() {
    let tmp = common::home_tempdir();
    let plugins = tmp.path().join("plugins");
    let mut large = b"plugin-title = Trop long\n".to_vec();
    // Past the bound (256 KiB): a comment is bytes as any.
    large.extend(std::iter::repeat_n(b"# padding\n".as_slice(), 30_000).flatten());
    let deep = format!(
        "plugin-title = {}profond{}\n",
        "{ ".repeat(40),
        " }".repeat(40)
    );
    let folder = plugin(
        &plugins,
        "word-count",
        &template(),
        &[
            ("en", template_english().as_bytes()),
            ("fr", &large),
            ("de", deep.as_bytes()),
            ("it", b"plugin-title = \xff\xfe\n"),
            ("es", b"plugin-title = Recuento\n"),
            // A name no language has, and a file that is no catalog.
            ("french", b"plugin-title = Nope\n"),
        ],
    );
    std::fs::write(folder.join("l10n").join("notes.txt"), "not a catalog").unwrap();
    // A pipe named as a catalog: never opened, which would wait for a
    // writer.
    #[cfg(unix)]
    {
        #[allow(clippy::disallowed_methods)] // A test's own pipe.
        let made = std::process::Command::new("mkfifo")
            .arg(folder.join("l10n").join("pt.ftl"))
            .status()
            .unwrap();
        assert!(made.success());
    }
    let app = app(&tmp.path().join("state"), &plugins);
    assert_eq!(
        skwir_core::words::languages_of("outside:word-count"),
        ["es"],
        "only the catalog within its bounds"
    );
    let (title, _) = listed(&app, "word-count");
    for (language, said) in [
        ("fr", "Word count"),
        ("de", "Word count"),
        ("it", "Word count"),
        ("es", "Recuento"),
    ] {
        assert_eq!(title.say_in(&[language.to_owned()]), said, "{language}");
    }
}
