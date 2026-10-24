//! What rclone says the storage of a mount Skwir made can do (ADR-086),
//! asked once a mount through its remote control (`operations/fsinfo`,
//! the socket `rclone_copy` copies through): whether it says how full it
//! is (else rclone's mount makes a size up, which the sidebar leaves
//! out), whether it makes a public link to a file (Copy Public Link), and
//! where it keeps what is deleted there (a cloud's own trash, which a
//! deletion's question then names). Never asked of a kind of storage by
//! its name alone: what rclone says it can do first.

use std::path::Path;

use serde_json::{Value, json};
use skwir_core::cancel::CancelToken;

#[cfg(unix)]
use crate::rclone_copy::{call, socket_for};

/// What a mount's storage can do, as rclone says.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Features {
    /// The mount's remote as rclone names it: `photos:`, or
    /// `:webdav{abc12}:` for one made of an address's parts.
    pub fs: String,
    /// It says how full it is (`About`).
    pub about: bool,
    /// It makes a public link to a file (`PublicLink`).
    pub public_link: bool,
    /// Where its server keeps what is deleted, in the user's words.
    pub trash: Option<String>,
}

/// Ask the mount Skwir made at `point` what its storage can do: `None`
/// where its remote control does not answer (a mount the user made, an
/// older rclone), and where Skwir reaches none (Windows, yet).
#[cfg(not(unix))]
pub fn of_mount(_point: &Path, _cancel: &CancelToken) -> Option<Features> {
    None
}

/// Ask the mount Skwir made at `point` what its storage can do: `None`
/// where its remote control does not answer (a mount the user made, an
/// older rclone).
#[cfg(unix)]
pub fn of_mount(point: &Path, cancel: &CancelToken) -> Option<Features> {
    let socket = socket_for(point)?;
    if !socket.exists() {
        return None;
    }
    let asked = || -> anyhow::Result<Features> {
        let listed = call(&socket, "vfs/list", &json!({}), cancel)?;
        let fs = listed["vfses"][0]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("no remote mounted"))?
            .to_owned();
        let info = call(&socket, "operations/fsinfo", &json!({ "fs": fs }), cancel)?;
        let features = &info["Features"];
        let has = |feature: &str| features[feature].as_bool().unwrap_or(false);
        // A remote of the configuration says its kind and options; one made
        // of an address's parts keeps no trash Skwir knows of.
        let trash = match configured(&fs) {
            Some(name) if has("CleanUp") => {
                let config = call(&socket, "config/get", &json!({ "name": name }), cancel)?;
                trash_of(&config)
            }
            _ => None,
        };
        Ok(Features {
            about: has("About"),
            public_link: has("PublicLink"),
            trash,
            fs,
        })
    };
    asked()
        .inspect_err(|error| {
            tracing::debug!("rclone's mount did not say what it can do: {error:#}");
        })
        .ok()
}

/// The configuration's remote `fs` names (`photos:` → `photos`); `None`
/// for one made of an address's parts (`:webdav,…:`).
#[cfg_attr(not(unix), allow(dead_code))]
fn configured(fs: &str) -> Option<&str> {
    let name = fs.split_once(':')?.0;
    (!name.is_empty()).then_some(name)
}

/// Where a remote of `config` (as `config/get` answers it: its `type` and
/// options) keeps what is deleted, for the kinds of storage whose trash
/// rclone empties (`CleanUp`), unless its options delete for good.
pub fn trash_of(config: &Value) -> Option<String> {
    let option = |name: &str| config[name].as_str().unwrap_or_default();
    let trash = match config["type"].as_str()? {
        "drive" if option("use_trash") != "false" => "Google Drive's trash",
        "onedrive" if option("hard_delete") != "true" => "OneDrive's recycle bin",
        "box" => "Box's trash",
        "pcloud" => "pCloud's trash",
        "mega" if option("use_trash") != "false" => "MEGA's rubbish bin",
        "jottacloud" if option("hard_delete") != "true" => "Jottacloud's trash",
        "yandex" if option("hard_delete") != "true" => "Yandex Disk's trash",
        _ => return None,
    };
    Some(trash.to_owned())
}

/// A public link to `path`: none where Skwir reaches no remote control
/// (Windows, yet).
///
/// # Errors
/// Always.
#[cfg(not(unix))]
pub fn public_link(_point: &Path, _path: &Path, _cancel: &CancelToken) -> anyhow::Result<String> {
    Err(skwir_core::system::not_yet("A public link").into())
}

/// A public link to `path`, a file of the mount Skwir made at `point`
/// (`operations/publiclink`).
///
/// # Errors
/// Where its remote control does not answer, or the storage refused.
#[cfg(unix)]
pub fn public_link(point: &Path, path: &Path, cancel: &CancelToken) -> anyhow::Result<String> {
    let socket = socket_for(point)
        .filter(|s| s.exists())
        .ok_or_else(|| anyhow::anyhow!("Skwir did not mount it, so rclone cannot be asked"))?;
    let listed = call(&socket, "vfs/list", &json!({}), cancel)?;
    let fs = listed["vfses"][0]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("no remote mounted"))?;
    let below = path
        .strip_prefix(point)?
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("rclone takes no such name"))?;
    let answer = call(
        &socket,
        "operations/publiclink",
        &json!({ "fs": fs, "remote": below }),
        cancel,
    )?;
    answer["url"]
        .as_str()
        .filter(|url| !url.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| anyhow::anyhow!("rclone made no link"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cloud_s_trash_is_named_unless_its_options_delete_for_good() {
        assert_eq!(
            trash_of(&json!({ "type": "drive" })).as_deref(),
            Some("Google Drive's trash")
        );
        assert_eq!(
            trash_of(&json!({ "type": "drive", "use_trash": "false" })),
            None
        );
        assert_eq!(
            trash_of(&json!({ "type": "onedrive", "hard_delete": "false" })).as_deref(),
            Some("OneDrive's recycle bin")
        );
        assert_eq!(
            trash_of(&json!({ "type": "onedrive", "hard_delete": "true" })),
            None
        );
        assert_eq!(trash_of(&json!({ "type": "webdav" })), None);
        assert_eq!(trash_of(&json!({})), None);
    }

    /// rclone's remote control as a mount serves it, answering `answer`
    /// to each call in turn, the calls heard sent back.
    #[cfg(unix)]
    fn stand_in(point: &Path, answers: Vec<Value>) -> std::sync::mpsc::Receiver<(String, Value)> {
        use std::io::{BufRead, BufReader, Read, Write};
        let socket = socket_for(point).unwrap();
        std::fs::create_dir_all(socket.parent().unwrap()).unwrap();
        let _ = std::fs::remove_file(&socket);
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let (heard, calls) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for answer in answers {
                let (stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream);
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let method = line
                    .split(' ')
                    .nth(1)
                    .unwrap()
                    .trim_start_matches('/')
                    .to_owned();
                let mut length = 0;
                loop {
                    let mut header = String::new();
                    reader.read_line(&mut header).unwrap();
                    if header.trim().is_empty() {
                        break;
                    }
                    if let Some(n) = header.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = n.trim().parse().unwrap();
                    }
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                heard
                    .send((method, serde_json::from_slice(&body).unwrap()))
                    .unwrap();
                let text = answer.to_string();
                let mut stream = reader.into_inner();
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{text}",
                    text.len()
                )
                .unwrap();
            }
            let _ = std::fs::remove_file(socket);
        });
        calls
    }

    /// A public link is asked of the mount's own remote control, for the
    /// file's path on the server.
    #[cfg(unix)]
    #[test]
    fn a_public_link_is_asked_of_the_mount_s_remote_control() {
        let point = Path::new("/nowhere/skwir-test-public-link");
        let calls = stand_in(
            point,
            vec![
                json!({ "vfses": ["box:"] }),
                json!({ "url": "https://example.com/s/it-s-shared" }),
            ],
        );
        let url = public_link(
            point,
            &point.join("a folder/it's here.txt"),
            &CancelToken::new(),
        )
        .unwrap();
        assert_eq!(url, "https://example.com/s/it-s-shared");
        let (method, _) = calls.recv().unwrap();
        assert_eq!(method, "vfs/list");
        let (method, args) = calls.recv().unwrap();
        assert_eq!(method, "operations/publiclink");
        assert_eq!(
            args,
            json!({ "fs": "box:", "remote": "a folder/it's here.txt" })
        );
    }

    /// What a mount can do is read from what rclone says: a cloud's trash
    /// where it empties one and its options keep it.
    #[cfg(unix)]
    #[test]
    fn a_mount_s_features_are_what_rclone_says() {
        let point = Path::new("/nowhere/skwir-test-features");
        let calls = stand_in(
            point,
            vec![
                json!({ "vfses": ["work:"] }),
                json!({ "Features": { "About": true, "PublicLink": true, "CleanUp": true } }),
                json!({ "type": "drive", "scope": "drive" }),
            ],
        );
        let features = of_mount(point, &CancelToken::new()).unwrap();
        assert_eq!(
            features,
            Features {
                fs: "work:".into(),
                about: true,
                public_link: true,
                trash: Some("Google Drive's trash".into()),
            }
        );
        let asked: Vec<String> = calls.try_iter().map(|(method, _)| method).collect();
        assert_eq!(asked, ["vfs/list", "operations/fsinfo", "config/get"]);
    }

    #[test]
    fn a_remote_of_the_configuration_is_told_from_an_address_s() {
        assert_eq!(configured("photos:"), Some("photos"));
        assert_eq!(configured("photos:2024/a"), Some("photos"));
        assert_eq!(configured(":webdav{AbC12}:"), None);
    }
}
