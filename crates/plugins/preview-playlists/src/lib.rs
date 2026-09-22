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
/// A `file:` URL naming another machine (`file://server/share/a.mp3`), or
/// anything with another scheme, is somewhere else. XSPF writes every
/// location as a URI, so a relative one there is escaped as a URI too.
fn place(base: &Path, location: &str, uri: bool) -> Place {
    let location = location.trim();
    if location.starts_with("file://") {
        return bibli_platform::uri::path_of(location).map_or(Place::Remote, Place::Local);
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
    let location = if uri {
        percent_encoding::percent_decode_str(location)
            .decode_utf8_lossy()
            .into_owned()
    } else {
        location.replace('\\', "/")
    };
    Place::Local(base.join(location))
}

/// The playlist's tracks, the first `limits::BLOCKS` of them, whether there
/// were more, and the encoding the text was read in.
struct Playlist {
    tracks: Vec<Track>,
    more: bool,
    encoding: inspection::Encoding,
    /// Locations are URIs (XSPF).
    uri: bool,
}
fn tracks(path: &Path, cancel: &CancelToken) -> anyhow::Result<Playlist> {
    let (source, encoding) = inspection::read_text(path, cancel)?;
    let format = inspection::extension(path);
    let mut tracks = match format.as_str() {
        "pls" => parse::pls(&source),
        "xspf" => parse::xspf(&source)?,
        "cue" => parse::cue(&source),
        _ => parse::m3u(&source),
    };
    let more = tracks.len() > limits::BLOCKS;
    tracks.truncate(limits::BLOCKS);
    Ok(Playlist {
        tracks,
        more,
        encoding,
        uri: format == "xspf",
    })
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
        let list = tracks(&path, cancel)?;
        let track = list
            .tracks
            .get(target.parse::<usize>()?)
            .ok_or_else(|| anyhow::anyhow!("No such track"))?;
        let Place::Local(file) = place(base, &track.location, list.uri) else {
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
        // The source is the shared code view (the worker's fallback).
        if view.source {
            return Ok(None);
        }
        let path = req.entry.resolve()?;
        let base = path.parent().unwrap_or(Path::new("."));
        let list = tracks(&path, cancel)?;
        let mut rows = Vec::with_capacity(list.tracks.len());
        let (mut total, mut missing, mut remote) = (0., 0, 0);
        for (index, track) in list.tracks.iter().enumerate() {
            inspection::check(cancel)?;
            let place = place(base, &track.location, list.uri);
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
        let details = inspection::details([
            (
                "Tracks",
                Some(if list.more {
                    format!("more than {}", rows.len())
                } else {
                    rows.len().to_string()
                }),
            ),
            ("Total length", (total > 0.).then(|| length(total))),
            ("Missing", inspection::count(missing)),
            ("Remote", inspection::count(remote)),
        ]);
        let notice = [
            list.encoding.notice(),
            list.more
                .then(|| format!("First {} tracks · Source shows the rest", limits::BLOCKS)),
            (remote > 0).then(|| "Remote entries are named, never fetched".to_owned()),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ");
        Ok(Some(Inspection {
            kind: InspectionKind::Table,
            title: format!("Playlist · {} tracks", rows.len()),
            columns: ["#", "Title", "Artist", "Length", ""]
                .map(String::from)
                .into(),
            rows,
            details,
            source_available: true,
            notice,
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
        let place = |base, location| place(base, location, false);
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
        // A share on another machine is not a path on this one.
        assert_eq!(place(base, "file://server/share/a.mp3"), Place::Remote);
        assert_eq!(
            place(base, "file://localhost/abs/f.ogg"),
            Place::Local("/abs/f.ogg".into())
        );
        // XSPF locations are URIs, relative ones included; an M3U's are paths.
        assert_eq!(
            super::place(base, "sub/caf%C3%A9%20b.ogg", true),
            Place::Local("/music/lists/sub/caf\u{e9} b.ogg".into())
        );
        assert_eq!(
            place(base, "100%25 mix.mp3"),
            Place::Local("/music/lists/100%25 mix.mp3".into())
        );
    }

    #[test]
    fn a_marked_legacy_or_long_playlist_is_read_whole_and_said() {
        let dir = tempfile::tempdir().unwrap();
        let read = |name: &str, bytes: &[u8]| {
            let list = dir.path().join(name);
            std::fs::write(&list, bytes).unwrap();
            let request = PreviewRequest {
                entry: bibli_core::op::EntryRef::from_path(&list).unwrap(),
                mime: Some("audio/x-mpegurl".into()),
                page: 1,
                seconds: None,
                max_side: 256,
            };
            Provider
                .inspect(&request, &InspectRequest::default(), &CancelToken::new())
                .unwrap()
                .unwrap()
        };
        // A byte order mark does not become a track named `#EXTM3U`.
        let data = read("marked.m3u8", "\u{feff}#EXTM3U\none.mp3\n".as_bytes());
        assert_eq!(data.rows.len(), 1);
        assert_eq!(data.rows[0].cells[1], "one");
        // Classic M3U is in the system code page.
        let data = read("legacy.m3u", b"#EXTINF:10,Caf\xe9 - Song\nx.mp3\n");
        assert_eq!(data.rows[0].cells[2], "Caf\u{e9}");
        assert_eq!(data.notice, "Read as Windows-1252");
        // A playlist longer than the bound shows its start and says so.
        let data = read("long.m3u", "a.mp3\n".repeat(limits::BLOCKS + 5).as_bytes());
        assert_eq!(data.rows.len(), limits::BLOCKS);
        assert!(
            data.notice.starts_with("First 2000 tracks"),
            "{}",
            data.notice
        );
        assert_eq!(data.details[0].value, "more than 2000");
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
