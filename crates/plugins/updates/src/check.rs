//! The request (ADR-142, 6): `GET <site>/updates/1.json`, conditional on
//! the last answer's tag, with a User-Agent of `skwir` alone and nothing
//! else about the copy: no version, system, language, identifier or
//! cookie. Only versions are read from the answer; no text and no address.

use std::time::Duration;

use serde::Deserialize;
use skwir_core::words;

use crate::version::Version;

/// The file's path on the site; `1` is its format.
pub const PATH: &str = "/updates/1.json";
/// How long the site is waited for.
pub const DEADLINE: Duration = Duration::from_secs(10);
/// The most read of its answer: a few dozen bytes are expected.
pub const MAX_BODY: usize = 16 * 1024;
/// What the request says it comes from.
pub const USER_AGENT: &str = "skwir";

/// What the site answered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fetched {
    /// The file as it was last read (304).
    Unchanged,
    /// The file, read anew, with its tag if it gave one.
    Read {
        answer: Answer,
        etag: Option<String>,
    },
}

/// The newest release, and the newest that fixes a loss of data or a hole
/// in security.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Answer {
    pub latest: Version,
    pub important_since: Option<Version>,
}

#[derive(Deserialize)]
struct File {
    latest: String,
    #[serde(default)]
    important_since: Option<String>,
}

impl Answer {
    /// The file's text read: refused whole where a version is not one
    /// (`Version::parse`). Fields it does not know are let be, for a later
    /// file of the same format.
    pub fn read(body: &[u8]) -> anyhow::Result<Answer> {
        let not_understood = || {
            anyhow::anyhow!(words!(
                updates_not_understood = "skwir.org's answer was not understood",
                note = "Why the check for a new version failed: the website's file holds no version Skwir reads."
            ))
        };
        let file: File = serde_json::from_slice(body).map_err(|_| not_understood())?;
        let latest = Version::parse(&file.latest).ok_or_else(not_understood)?;
        let important_since = match file.important_since {
            None => None,
            Some(text) => Some(Version::parse(&text).ok_or_else(not_understood)?),
        };
        Ok(Answer {
            latest,
            important_since,
        })
    }
}

/// Ask `site` for the file, conditionally on `etag`, on a runtime of the
/// request's own: blocks for `DEADLINE` at most. Call it on a thread of
/// its own.
pub fn fetch(site: &str, etag: Option<&str>) -> anyhow::Result<Fetched> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let http = skwir_platform::http::Http::system(USER_AGENT)?.limited(DEADLINE, MAX_BODY);
    let headers: Vec<(String, String)> = etag
        .map(|etag| ("If-None-Match".to_owned(), etag.to_owned()))
        .into_iter()
        .collect();
    let url = format!("{}{PATH}", site.trim_end_matches('/'));
    let answer = runtime.block_on(http.get(&url, &headers))?;
    match answer.status {
        304 => Ok(Fetched::Unchanged),
        200 => Ok(Fetched::Read {
            answer: Answer::read(&answer.body)?,
            etag: answer
                .headers
                .get("etag")
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned),
        }),
        status => Err(anyhow::anyhow!(words!(
            updates_status = "skwir.org answered { $status }",
            status = status.to_string(),
            note = "Why the check for a new version failed: the website's answer, an HTTP status (404)."
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::Answer;
    use crate::version::Version;

    #[test]
    fn only_versions_are_read_and_a_wrong_one_refuses_the_file() {
        let read = |text: &str| Answer::read(text.as_bytes());
        assert_eq!(
            read(r#"{"latest": "0.6.2", "important_since": "0.6.1"}"#).unwrap(),
            Answer {
                latest: Version(0, 6, 2),
                important_since: Some(Version(0, 6, 1)),
            }
        );
        assert_eq!(
            read(r#"{"latest": "0.6.2", "later": {"what": "a field to come"}}"#)
                .unwrap()
                .important_since,
            None
        );
        for wrong in [
            "",
            "{}",
            "[]",
            r#"{"latest": "v0.6.2"}"#,
            r#"{"latest": 6}"#,
            r#"{"latest": "0.6.2", "important_since": "soon"}"#,
            r#"{"latest": "0.6.2", "important_since": 1}"#,
        ] {
            assert!(read(wrong).is_err(), "{wrong:?}");
        }
    }
}
