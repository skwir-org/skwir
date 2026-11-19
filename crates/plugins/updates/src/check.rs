//! The request (ADR-142, 6): `GET <site>/updates/1.json`, with a
//! User-Agent of `skwir` alone and nothing else about the copy: no
//! version, system, language, identifier or cookie. It is not made
//! conditional on the last answer's tag: a tag the server chose, sent
//! back each day, could tell one copy from another, and the file is a few
//! dozen bytes. Only versions are read from the answer; no text and no
//! address.

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
    pub fn read(body: &[u8]) -> Option<Answer> {
        let file: File = serde_json::from_slice(body).ok()?;
        let important_since = match file.important_since {
            None => None,
            Some(text) => Some(Version::parse(&text)?),
        };
        Some(Answer {
            latest: Version::parse(&file.latest)?,
            important_since,
        })
    }
}

/// Ask `site` for the file, on a runtime of the request's own: blocks for
/// `DEADLINE` at most, the request whole (`Http::get`), and the runtime is
/// let go without waiting for what it still runs (a name the system is
/// still asked for). Any answer but the file (a redirect included, which
/// is not followed) is a failure. Call it on a thread of its own.
pub fn fetch(site: &str) -> anyhow::Result<Answer> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let http = skwir_platform::http::Http::system(USER_AGENT)?.limited(DEADLINE, MAX_BODY);
    let url = format!("{}{PATH}", site.trim_end_matches('/'));
    let answer = runtime.block_on(http.get(&url, &[]));
    runtime.shutdown_background();
    let answer = answer?;
    let site = crate::host(site);
    match answer.status {
        200 => Answer::read(&answer.body).ok_or_else(|| {
            anyhow::anyhow!(words!(
                updates_not_understood = "{ $site }'s answer was not understood",
                site = site,
                note = "Why the check for a new version failed: the website's file holds no version Skwir reads. site: the website, as written (skwir.org)."
            ))
        }),
        status => Err(anyhow::anyhow!(words!(
            updates_status = "{ $site } answered { $status }",
            site = site,
            status = status.to_string(),
            note = "Why the check for a new version failed: the website's answer, an HTTP status (404). site: the website, as written (skwir.org)."
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
            assert!(read(wrong).is_none(), "{wrong:?}");
        }
    }
}
