//! Where words typed in the address field search (ADR-098), with the
//! plugins the binary registers: a place is a scope where its provider says
//! words can search it (`LocationProvider::searchable`), never by its
//! scheme, and the field's Search button is offered while a source
//! searches words (`SuggestSource::searches`), never by a command's name.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use skwir_core::app::{App, AppConfig};
use skwir_core::cancel::CancelToken;
use skwir_core::location::Location;
use skwir_core::suggest::{Ask, Typed};
use skwir_plugin_tags::location::Address;
use skwir_ui::address::search_title;
use skwir_ui::suggestions::scope_of;

fn app(state: &Path) -> Arc<App> {
    let app = App::new(AppConfig::ephemeral(state)).unwrap();
    app.plugins
        .add(Arc::new(skwir_plugin_query::QueryPlugin::default()), &app);
    app.plugins
        .add(Arc::new(skwir_plugin_jump::JumpPlugin::default()), &app);
    app.plugins
        .add(Arc::new(skwir_plugin_trash::TrashPlugin::default()), &app);
    app.plugins
        .add(Arc::new(skwir_plugin_tags::TagsPlugin::default()), &app);
    app.plugins
        .add(Arc::new(skwir_plugin_shelf::ShelfPlugin::default()), &app);
    skwir_ui::suggestions::register(&app);
    app
}

fn tag(name: &str) -> Location {
    Address {
        tag: Some(name.to_owned()),
        ..Address::default()
    }
    .location()
}

/// A folder, a tag and Recent are searched; every tag, the trash and the
/// shelf are no scope, nor is a search, which is edited; with the query
/// plugin off nothing searches words, and the button goes, though Ctrl+F
/// still takes a folder as the scope.
#[test]
fn a_place_is_a_scope_where_its_provider_says_words_search_it() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp.path().join("state"));
    let folder = Location::from_path(tmp.path().join("it's ünder"));
    let france = tag("Places/l'Été");
    let all_tags = Address::default().location();
    let title = |location: &Location| search_title(&app, Some(location), false);

    assert_eq!(
        title(&folder).as_deref(),
        Some("Search this folder and its subfolders")
    );
    assert_eq!(title(&france).as_deref(), Some("Search l'Été"));
    assert_eq!(
        title(&Location::parse("recent:")).as_deref(),
        Some("Search Recent")
    );
    for none in [
        all_tags.clone(),
        // The scheme's own row (`tags:` typed), every tag too.
        Location::new("tags", ""),
        Location::parse("trash:"),
        Location::parse("shelf:"),
        Location::new("query", "dunes"),
    ] {
        assert_eq!(title(&none), None, "{none}");
        assert_eq!(scope_of(&app, &none), None, "{none}");
    }
    assert_eq!(scope_of(&app, &france), Some(france.clone()));
    assert_eq!(
        search_title(&app, Some(&Location::new("query", "dunes")), true).as_deref(),
        Some("Edit this search")
    );

    assert!(app.plugins.set_enabled("query", false, &app));
    assert_eq!(title(&folder), None, "nothing searches words");
    assert_eq!(search_title(&app, Some(&folder), true), None);
    assert_eq!(scope_of(&app, &folder), Some(folder.clone()));
    assert!(app.plugins.set_enabled("query", true, &app));
    assert!(title(&folder).is_some());
}

/// The sidebar's places the field offers for words carry a scope where
/// words can search them (Ctrl+F on the row): Recent's, never every tag's
/// nor the trash's.
#[test]
fn the_sidebars_places_offered_carry_a_scope_only_where_words_search() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(&tmp.path().join("state"));
    let deadline = Instant::now() + Duration::from_secs(10);
    while app
        .registry
        .places
        .find(|p| p.location == Address::default().location())
        .is_none()
    {
        assert!(Instant::now() < deadline, "All Tags never shown");
        app.pump();
        std::thread::sleep(Duration::from_millis(20));
    }
    let scope_of_row = |words: &str, title: &str| {
        let ask = Ask {
            text: words.to_owned(),
            caret: words.len(),
            typed: Typed::Words,
            scope: None,
            shown: None,
            dir: None,
            pane: None,
        };
        let answer = skwir_core::suggest::gather(&app, &ask, &CancelToken::new());
        answer
            .rows
            .into_iter()
            .find(|row| row.source == "places" && row.title == title)
            .unwrap_or_else(|| panic!("no row {title} for {words}"))
            .scope
    };
    assert_eq!(
        scope_of_row("recen", "Recent"),
        Some(Location::parse("recent:"))
    );
    assert_eq!(scope_of_row("all tag", "All Tags"), None);
    assert_eq!(scope_of_row("tras", "Trash"), None);
}
