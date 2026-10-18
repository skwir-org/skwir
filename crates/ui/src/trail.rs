//! What the location a pane shows says of itself when it is no folder
//! (ADR-032): its trail of places (`LocationProvider::trail`: a tag
//! location reads `Home › Tags › Places › France`, then what narrows it,
//! drawn with the location's glyph on Tags and each tag as its chip;
//! the trash, one place), what gestures mean there
//! (`LocationProvider::verbs`: Delete takes a file out of a tag or the
//! shelf, a drop gives files the tag, New Folder makes a tag), and what
//! it says when it lists nothing (`LocationProvider::empty_state`, with
//! its `icon`). Read once per location a pane shows, outside the
//! workspace lock, never per frame. The shell never asks a location's
//! scheme how to show it: a new kind of location needs no branch here.

use skwir_core::location::{Crumb, EmptyState, Location, Trail, Verbs};

use crate::shell::Shell;

/// One pane's: what was read for the location it shows.
#[derive(Default)]
pub struct Shown {
    /// The location they were read for, as text.
    location: String,
    /// What it is called, as its tab says it (`App::location_title`):
    /// the window's title and the name assistive technology gives the
    /// pane's entries.
    pub title: gpui::SharedString,
    pub trail: Option<Trail>,
    pub verbs: Verbs,
    /// The glyph that stands for it (`LocationProvider::icon`).
    pub icon: Option<&'static str>,
    /// What it says when it lists nothing (`LocationProvider::empty_state`).
    pub empty: Option<EmptyState>,
    /// It is a folder: what it lists is in it (no Folder column).
    pub folder: bool,
    /// It is a search (`LocationProvider::searches`): results, pills.
    pub search: bool,
    /// The trail's places that take files dropped on them (`Verbs::drop`).
    takes_drops: Vec<Location>,
    /// The sidebar's arrangement they were named under (`Roots::generation`).
    names: u64,
}

impl Shown {
    /// Whether `place`, one of the trail's, takes files dropped on it.
    pub fn takes_drops(&self, place: &Location) -> bool {
        self.takes_drops.contains(place)
    }
}

/// Read the trail and verbs of the location the pane being rendered
/// shows, when it shows another one than they were read for.
pub fn sync(shell: &mut Shell) {
    let location = shell.app.pane_location(shell.pane);
    let text = location
        .as_ref()
        .map(ToString::to_string)
        .unwrap_or_default();
    let names = shell.roots.generation;
    if shell.ui().place.location == text && shell.ui().place.names == names {
        return;
    }
    let (mut trail, verbs, icon, empty) = match &location {
        Some(location) if !location.is_path() => (
            shell.app.trail(location),
            shell.app.verbs(location),
            shell.app.location_icon(location),
            shell.app.empty_state(location),
        ),
        // A folder is a trail of its own: its folders from the sidebar's
        // place that holds it, the last holding folders.
        Some(location) => (
            location.path().map(|base| Trail {
                base: Some(base),
                holds_places: true,
                ..Trail::default()
            }),
            Verbs::default(),
            None,
            None,
        ),
        None => (None, Verbs::default(), None, None),
    };
    // A place the user renamed in the sidebar reads as they named it
    // (the Trash they call Bin), as its tab and the history say it.
    let registry = &shell.app.registry;
    for crumb in trail.iter_mut().flat_map(|trail| &mut trail.crumbs) {
        if let Some(title) = registry
            .places
            .find(|p| p.location.same_place(&crumb.location))
            .and_then(|place| registry.user_title(&place))
        {
            crumb.label = title;
        }
    }
    let takes_drops = trail
        .iter()
        .flat_map(|trail| &trail.crumbs)
        .filter(|crumb| shell.app.verbs(&crumb.location).drop.is_some())
        .map(|crumb| crumb.location.clone())
        .collect();
    let search = location
        .as_ref()
        .is_some_and(|location| shell.app.is_search(location));
    let title = location
        .as_ref()
        .map(|location| shell.app.location_title(location))
        .unwrap_or_default()
        .into();
    shell.ui_mut().place = Shown {
        folder: location.as_ref().is_none_or(Location::is_path),
        search,
        location: text,
        title,
        trail,
        verbs,
        icon,
        empty,
        takes_drops,
        names,
    };
}

/// The glyph that stands for `location` where it is named among others (a
/// tab, the history, a menu of places): its provider's
/// (`LocationProvider::icon`), else a folder's.
pub fn glyph(app: &skwir_core::App, location: &Location) -> &'static str {
    app.location_icon(location).unwrap_or("folder")
}

/// The segments of a trail as the address draws them: the folders of
/// its base, each its own place, from the sidebar's that holds it
/// (`address::crumbs`), then its crumbs.
pub fn segments(trail: &Trail, roots: &[crate::sidebar::Root]) -> Vec<Crumb> {
    let mut out: Vec<Crumb> = trail
        .base
        .as_deref()
        .map(|base| {
            crate::address::crumbs(base, roots)
                .into_iter()
                .map(|(label, path)| Crumb::new(label, Location::from_path(path)))
                .collect()
        })
        .unwrap_or_default();
    out.extend(trail.crumbs.iter().cloned());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_trail_continues_the_folder_it_is_of() {
        let trail = Trail {
            base: Some("/home/someone/Photos".into()),
            crumbs: vec![
                Crumb::new("Tags", Location::new("tags", "/?in=x")),
                Crumb::new("Places", Location::new("tags", "/Places?in=x")),
            ],
            filters: Vec::new(),
            toggles: Vec::new(),
            holds_places: true,
            one_folder: false,
        };
        let home = crate::sidebar::Root {
            path: "/home/someone".into(),
            name: "Home".into(),
        };
        let labels: Vec<String> = segments(&trail, &[home])
            .into_iter()
            .map(|c| c.label)
            .collect();
        assert_eq!(labels, ["Home", "Photos", "Tags", "Places"]);
        assert_eq!(trail.parent(), Some(Location::new("tags", "/?in=x")));
        let first = Trail {
            crumbs: trail.crumbs[..1].to_vec(),
            ..trail
        };
        assert_eq!(
            first.parent(),
            Some(Location::from_path("/home/someone/Photos")),
            "Up from a tag location's top goes to the folder it is of"
        );
    }
}
