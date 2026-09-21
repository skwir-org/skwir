//! Playlists (M3U, PLS, XSPF, CUE) as a list of tracks. A track that is a
//! file on this machine opens in the panel as what it is, the player, and the
//! player's Previous and Next then walk the playlist. A playlist is read, not
//! obeyed: nothing is fetched, and a remote entry is only named.
mod parse;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bibli_core::{
    App, CancelToken,
    inspection::{InspectRequest, Inspection, InspectionKind, Row},
    plugin::Plugin,
    preview::{Member, PreviewProvider, PreviewRequest},
};
use bibli_platform::inspection::{self, limits};
use parse::Track;

pub struct PreviewPlugin;
impl Plugin for PreviewPlugin {
    fn id(&self) -> &'static str {
        "preview-playlists"
    }
    fn title(&self) -> &'static str {
        "Preview: playlists"
    }
    fn description(&self) -> &'static str {
        "List the tracks of M3U, PLS, XSPF and CUE files, and play the ones that are on this machine. Remote entries are named, never fetched"
    }
    fn register(&self, app: &App) {
        app.previews.register(self.id(), Arc::new(Provider));
    }
}

/// Where a playlist's entry leads.
#[derive(Debug, PartialEq)]
enum Place {
    Local(PathBuf),
    Remote,
}

/// A location as playlists write them: a path next to the playlist, an
/// absolute one, a `file:` URL, sometimes with a Windows player's backslashes.
/// Anything with another scheme is somewhere else.
fn place(base: &Path, location: &str) -> Place {
    let location = location.trim();
    if let Some(path) = location.strip_prefix("file://") {
        let path = path.strip_prefix("localhost").unwrap_or(path);
        let decoded = percent_encoding::percent_decode_str(path).decode_utf8_lossy();
        return Place::Local(PathBuf::from(decoded.as_ref()));
    }
    let scheme = location.split_once("://").is_some_and(|(scheme, _)| {
        scheme.len() > 1
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '+')
    });
    if scheme {
        return Place::Remote;
    }
    Place::Local(base.join(location.replace('\\', "/")))
}

fn tracks(path: &Path, cancel: &CancelToken) -> anyhow::Result<Vec<Track>> {
    let source = inspection::text(path, cancel)?;
    let mut tracks = match inspection::extension(path).as_str() {
        "pls" => parse::pls(&source),
        "xspf" => parse::xspf(&source)?,
        "cue" => parse::cue(&source),
        _ => parse::m3u(&source),
    };
    tracks.truncate(limits::BLOCKS);
    Ok(tracks)
}

fn length(seconds: f64) -> String {
    let total = seconds.round() as u64;
    if total >= 3600 {
        format!("{}:{:02}:{:02}", total / 3600, total / 60 % 60, total % 60)
    } else {
        format!("{}:{:02}", total / 60, total % 60)
    }
}

struct Provider;
impl PreviewProvider for Provider {
    fn id(&self) -> &'static str {
        "preview-playlists"
    }
    fn accepts(&self, req: &PreviewRequest) -> bool {
        matches!(
            inspection::extension(&req.entry.path).as_str(),
            "m3u" | "m3u8" | "pls" | "xspf" | "cue"
        )
    }
    fn extract(
        &self,
        req: &PreviewRequest,
        target: &str,
        _into: &Path,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<Member>> {
        let path = req.entry.resolve()?;
        let base = path.parent().unwrap_or(Path::new("."));
        let tracks = tracks(&path, cancel)?;
        let track = tracks
            .get(target.parse::<usize>()?)
            .ok_or_else(|| anyhow::anyhow!("No such track"))?;
        let Place::Local(file) = place(base, &track.location) else {
            anyhow::bail!("This track is not on this machine; previews fetch nothing")
        };
        anyhow::ensure!(file.is_file(), "{} is missing", file.display());
        Ok(Some(Member {
            name: file.file_name().map_or_else(
                || track.location.clone(),
                |name| name.to_string_lossy().into_owned(),
            ),
            path: file,
            // The track is a file of its own; the playlist only names it.
            own_file: true,
        }))
    }
    fn inspect(
        &self,
        req: &PreviewRequest,
        view: &InspectRequest,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<Inspection>> {
        let path = req.entry.resolve()?;
        if view.source {
            return inspection::source(&path, view.offset, cancel).map(Some);
        }
        let base = path.parent().unwrap_or(Path::new("."));
        let tracks = tracks(&path, cancel)?;
        let mut rows = Vec::with_capacity(tracks.len());
        let (mut total, mut missing, mut remote) = (0., 0, 0);
        for (index, track) in tracks.iter().enumerate() {
            inspection::check(cancel)?;
            let place = place(base, &track.location);
            let here = matches!(&place, Place::Local(file) if file.is_file());
            let note = match (&place, here) {
                (Place::Remote, _) => {
                    remote += 1;
                    "remote"
                }
                (Place::Local(_), false) => {
                    missing += 1;
                    "missing"
                }
                (Place::Local(_), true) => "",
            };
            let title = if track.title.is_empty() {
                Path::new(&track.location.replace('\\', "/"))
                    .file_stem()
                    .map_or_else(
                        || track.location.clone(),
                        |stem| stem.to_string_lossy().into_owned(),
                    )
            } else {
                track.title.clone()
            };
            total += track.seconds.unwrap_or(0.);
            rows.push(Row {
                cells: vec![
                    (index + 1).to_string(),
                    title.chars().take(limits::TABLE_CELL).collect(),
                    track.artist.chars().take(limits::TABLE_CELL).collect(),
                    track.seconds.map(length).unwrap_or_default(),
                    note.into(),
                ],
                // Only what can be shown is a way in.
                target: here.then(|| index.to_string()),
                ..Row::default()
            });
        }
        let details = [
            ("Tracks", rows.len().to_string()),
            (
                "Total length",
                if total > 0. {
                    length(total)
                } else {
                    String::new()
                },
            ),
            (
                "Missing",
                if missing > 0 {
                    missing.to_string()
                } else {
                    String::new()
                },
            ),
            (
                "Remote",
                if remote > 0 {
                    remote.to_string()
                } else {
                    String::new()
                },
            ),
        ]
        .into_iter()
        .filter_map(|(label, value)| inspection::detail(label, value))
        .collect();
        Ok(Some(Inspection {
            kind: InspectionKind::Table,
            title: format!("Playlist · {} tracks", rows.len()),
            columns: ["#", "Title", "Artist", "Length", ""]
                .map(String::from)
                .into(),
            rows,
            details,
            source_available: true,
            notice: if remote > 0 {
                "Remote entries are named, never fetched".into()
            } else {
                String::new()
            },
            ..Inspection::default()
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_location_is_a_file_here_or_somewhere_else() {
        let base = Path::new("/music/lists");
        assert_eq!(
            place(base, "a/b.mp3"),
            Place::Local("/music/lists/a/b.mp3".into())
        );
        assert_eq!(
            place(base, "..\\other\\c.flac"),
            Place::Local("/music/lists/../other/c.flac".into())
        );
        assert_eq!(place(base, "/abs/d.ogg"), Place::Local("/abs/d.ogg".into()));
        assert_eq!(
            place(base, "file:///music/it's%20a%20caf%C3%A9.mp3"),
            Place::Local("/music/it's a café.mp3".into())
        );
        assert_eq!(place(base, "https://example.invalid/stream"), Place::Remote);
        assert_eq!(place(base, "mms://example.invalid/x"), Place::Remote);
        // A drive letter is not a scheme worth fetching, and there is no `//`.
        assert!(matches!(place(base, "C:\\Music\\e.mp3"), Place::Local(_)));
    }

    #[test]
    fn a_playlist_lists_its_tracks_and_opens_only_what_is_here() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("music")).unwrap();
        std::fs::write(
            dir.path().join("music/it's a café.mp3"),
            b"not really audio",
        )
        .unwrap();
        let list = dir.path().join("mix.m3u8");
        std::fs::write(
            &list,
            "#EXTM3U\n#EXTINF:75,Someone - A Song\nmusic/it's a café.mp3\ngone.mp3\nhttps://example.invalid/radio\n",
        )
        .unwrap();
        let request = PreviewRequest {
            entry: bibli_core::op::EntryRef::from_path(&list).unwrap(),
            mime: None,
            page: 1,
            seconds: None,
            max_side: 256,
        };
        let cancel = CancelToken::new();
        let data = Provider
            .inspect(&request, &InspectRequest::default(), &cancel)
            .unwrap()
            .unwrap();
        let cells: Vec<_> = data.rows.iter().map(|row| row.cells.join("|")).collect();
        assert_eq!(
            cells,
            [
                "1|A Song|Someone|1:15|",
                "2|gone|||missing",
                "3|radio|||remote"
            ]
        );
        let targets: Vec<_> = data.rows.iter().map(|row| row.target.as_deref()).collect();
        assert_eq!(targets, [Some("0"), None, None]);
        assert!(!data.notice.is_empty());

        let member = Provider
            .extract(&request, "0", dir.path(), &cancel)
            .unwrap()
            .unwrap();
        assert_eq!(member.name, "it's a café.mp3");
        assert!(member.own_file && member.path.ends_with("music/it's a café.mp3"));
        assert!(
            Provider
                .extract(&request, "1", dir.path(), &cancel)
                .is_err(),
            "missing"
        );
        assert!(
            Provider
                .extract(&request, "2", dir.path(), &cancel)
                .is_err(),
            "remote"
        );
    }
}
