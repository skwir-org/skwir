//! The four playlist formats people keep: M3U, PLS, XSPF and CUE sheets.
//! Pure text in, tracks out; nothing here touches the filesystem.

/// One track as the playlist names it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Track {
    /// A path, relative to the playlist or absolute, or a URL. Never opened
    /// here: the provider decides what it may be.
    pub location: String,
    pub title: String,
    pub artist: String,
    pub seconds: Option<f64>,
}

pub fn m3u(source: &str) -> Vec<Track> {
    let mut tracks = Vec::new();
    let mut pending = Track::default();
    for line in source
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        if let Some(info) = line.strip_prefix("#EXTINF:") {
            // `#EXTINF:215 key=value,Artist - Title`
            let (length, label) = info.split_once(',').unwrap_or((info, ""));
            pending.seconds = length
                .split_whitespace()
                .next()
                .and_then(|seconds| seconds.parse::<f64>().ok())
                .filter(|seconds| *seconds > 0.);
            match label.split_once(" - ") {
                Some((artist, title)) => {
                    artist.trim().clone_into(&mut pending.artist);
                    title.trim().clone_into(&mut pending.title);
                }
                None => label.trim().clone_into(&mut pending.title),
            }
        } else if !line.starts_with('#') {
            line.clone_into(&mut pending.location);
            tracks.push(std::mem::take(&mut pending));
        }
    }
    tracks
}

pub fn pls(source: &str) -> Vec<Track> {
    let mut tracks: Vec<Track> = Vec::new();
    for line in source.lines().map(str::trim) {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let digits = key.trim_start_matches(|c: char| c.is_ascii_alphabetic());
        let Some(index) = digits.parse::<usize>().ok().and_then(|n| n.checked_sub(1)) else {
            continue;
        };
        if index >= 100_000 {
            continue;
        }
        if tracks.len() <= index {
            tracks.resize_with(index + 1, Track::default);
        }
        let value = value.trim();
        match &key[..key.len() - digits.len()] {
            "file" => value.clone_into(&mut tracks[index].location),
            "title" => value.clone_into(&mut tracks[index].title),
            "length" => {
                tracks[index].seconds = value.parse::<f64>().ok().filter(|s| *s > 0.);
            }
            _ => {}
        }
    }
    tracks.retain(|track| !track.location.is_empty());
    tracks
}

pub fn xspf(source: &str) -> anyhow::Result<Vec<Track>> {
    bibli_platform::inspection::xml_depth(source.as_bytes())?;
    let document = roxmltree::Document::parse(source)?;
    let text = |node: roxmltree::Node<'_, '_>, name: &str| {
        node.children()
            .find(|child| child.tag_name().name() == name)
            .and_then(|child| child.text())
            .map(|text| text.trim().to_owned())
            .unwrap_or_default()
    };
    Ok(document
        .descendants()
        .filter(|node| node.tag_name().name() == "track")
        .map(|track| Track {
            location: text(track, "location"),
            title: text(track, "title"),
            artist: text(track, "creator"),
            seconds: text(track, "duration")
                .parse::<f64>()
                .ok()
                .filter(|ms| *ms > 0.)
                .map(|ms| ms / 1000.),
        })
        .filter(|track| !track.location.is_empty())
        .collect())
}

/// A cue sheet cuts one recording into tracks: each names the file it is in
/// and where it starts. A track's length is the distance to the next one in
/// the same file; the last one's is not written anywhere.
pub fn cue(source: &str) -> Vec<Track> {
    let quoted = |rest: &str| {
        let rest = rest.trim();
        rest.strip_prefix('"')
            .and_then(|rest| rest.split_once('"'))
            .map_or_else(
                || rest.split_whitespace().next().unwrap_or("").to_owned(),
                |(inside, _)| inside.to_owned(),
            )
    };
    let mut album_artist = String::new();
    let mut file = String::new();
    // A track, the file it is in and where it starts, in seconds.
    let mut tracks: Vec<(Track, Option<f64>)> = Vec::new();
    for line in source.lines().map(str::trim) {
        let (word, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        match word.to_ascii_uppercase().as_str() {
            "FILE" => file = quoted(rest),
            "TRACK" => tracks.push((
                Track {
                    location: file.clone(),
                    ..Track::default()
                },
                None,
            )),
            "TITLE" => {
                if let Some((track, _)) = tracks.last_mut() {
                    track.title = quoted(rest);
                }
            }
            "PERFORMER" => match tracks.last_mut() {
                Some((track, _)) => track.artist = quoted(rest),
                None => album_artist = quoted(rest),
            },
            "INDEX" if rest.trim_start().starts_with("01") => {
                // mm:ss:ff, seventy-five frames to the second
                let stamp: Vec<f64> = rest
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("")
                    .split(':')
                    .filter_map(|part| part.parse().ok())
                    .collect();
                if let ([minutes, seconds, frames], Some((_, start))) =
                    (stamp.as_slice(), tracks.last_mut())
                {
                    *start = Some(minutes * 60. + seconds + frames / 75.);
                }
            }
            _ => {}
        }
    }
    let starts: Vec<(String, Option<f64>)> = tracks
        .iter()
        .map(|(track, start)| (track.location.clone(), *start))
        .collect();
    tracks
        .into_iter()
        .enumerate()
        .map(|(index, (mut track, start))| {
            if track.artist.is_empty() {
                track.artist.clone_from(&album_artist);
            }
            track.seconds = starts
                .get(index + 1)
                .filter(|(file, _)| *file == track.location)
                .and_then(|(_, next)| Some(next.as_ref()? - start?))
                .filter(|seconds| *seconds > 0.);
            track
        })
        .filter(|track| !track.location.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn m3u_reads_plain_and_extended_lines() {
        let tracks = m3u(
            "#EXTM3U\r\n#EXTINF:215,The Test Tones - It's a café\r\nmusic/01 café.mp3\r\n\r\n# a comment\r\n../other/second.flac\r\n#EXTINF:-1,Radio\r\nhttps://example.invalid/stream\r\n",
        );
        assert_eq!(tracks.len(), 3);
        assert_eq!(
            tracks[0],
            Track {
                location: "music/01 café.mp3".into(),
                title: "It's a café".into(),
                artist: "The Test Tones".into(),
                seconds: Some(215.),
            }
        );
        assert_eq!(tracks[1].location, "../other/second.flac");
        assert!(tracks[1].title.is_empty() && tracks[1].seconds.is_none());
        assert_eq!(
            (tracks[2].title.as_str(), tracks[2].seconds),
            ("Radio", None)
        );
    }

    #[test]
    fn pls_pairs_entries_by_number_whatever_their_order() {
        let tracks = pls(
            "[playlist]\nTitle2=Second\nFile1=one.ogg\nFile2=two.ogg\nLength1=61\nLength2=-1\nTitle1=First\nNumberOfEntries=2\nFile99999999=x\n",
        );
        assert_eq!(tracks.len(), 2);
        assert_eq!(
            (
                tracks[0].location.as_str(),
                tracks[0].title.as_str(),
                tracks[0].seconds
            ),
            ("one.ogg", "First", Some(61.))
        );
        assert_eq!(
            (tracks[1].title.as_str(), tracks[1].seconds),
            ("Second", None)
        );
    }

    #[test]
    fn xspf_reads_tracks_and_rejects_what_is_not_xml() {
        let tracks = xspf(r#"<?xml version="1.0"?><playlist xmlns="http://xspf.org/ns/0/"><trackList><track><location>file:///music/a%20b.mp3</location><title>A B</title><creator>Someone</creator><duration>90500</duration></track><track><title>No location</title></track></trackList></playlist>"#).unwrap();
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].location, "file:///music/a%20b.mp3");
        assert_eq!(
            (tracks[0].artist.as_str(), tracks[0].seconds),
            ("Someone", Some(90.5))
        );
        assert!(xspf("not xml").is_err());
    }

    #[test]
    fn a_cue_sheet_cuts_one_file_into_tracks() {
        let tracks = cue(
            "PERFORMER \"The Test Tones\"\nTITLE \"Live\"\nFILE \"live set.flac\" WAVE\n  TRACK 01 AUDIO\n    TITLE \"Opening\"\n    INDEX 01 00:00:00\n  TRACK 02 AUDIO\n    TITLE \"Second\"\n    PERFORMER \"A Guest\"\n    INDEX 00 03:58:00\n    INDEX 01 04:00:37\n",
        );
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0].location, "live set.flac");
        assert_eq!(
            (tracks[0].title.as_str(), tracks[0].artist.as_str()),
            ("Opening", "The Test Tones")
        );
        assert!((tracks[0].seconds.unwrap() - (240. + 37. / 75.)).abs() < 1e-9);
        assert_eq!(
            (tracks[1].artist.as_str(), tracks[1].seconds),
            ("A Guest", None)
        );
    }
}
