//! How a mount Skwir makes is made, as the user chose for its server
//! (ADR-086): read-only, rclone's file cache and bandwidth, sshfs's
//! compression. Kept in the plugin's data (`remote.options`, by the
//! server's address, beside the thumbnails chosen for each), never in a
//! tool's configuration; given to the tool when it next connects. GVfs
//! and KIO mount as the desktop does: they take none.

use std::collections::BTreeMap;

use skwir_core::app::App;
use skwir_core::form::{Choice, Field, Form, Kind, Values};

use crate::address::{Address, Scheme};

/// The plugin's data: each server's options, by its address's top.
pub const SETTING: &str = "remote.options";

/// What a mount is made with.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MountOptions {
    /// Nothing is written there.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub read_only: bool,
    /// rclone's file cache (`--vfs-cache-mode`): `off`, `minimal`,
    /// `writes`, `full`, each kept once chosen; `None` for Automatic
    /// (`automatic`, ADR-102).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache: Option<String>,
    /// rclone's bandwidth limit (`--bwlimit`): `10M`, `1M:512k`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bandwidth: Option<String>,
    /// ssh's compression, for sshfs.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub compression: bool,
}

/// rclone's file caches, as it names them, and what each does.
const CACHES: [(&str, &str); 4] = [
    (
        "full",
        "What is read and written is kept on this disk an hour: read again from it, sent after it is closed, tried again",
    ),
    (
        "writes",
        "Files written are kept on this disk and sent after they are closed; each read asks the server",
    ),
    (
        "minimal",
        "Each read asks the server; a file written whole is on it once closed",
    ),
    (
        "off",
        "Nothing kept on this disk: some programs cannot write",
    ),
];

/// The cache Automatic chooses for a server (ADR-102), and why: `full`,
/// which keeps what is read and written, but `minimal` for a remote that
/// encrypts or wraps others (`kind`, rclone's type: `full` would keep
/// their files on this disk as read) and on the local network (`near`),
/// where a read costs little and a server is expected to have a file
/// written once it is closed.
pub fn automatic(kind: Option<&str>, near: Option<bool>) -> (&'static str, &'static str) {
    if kind.is_some_and(crate::rclone_forms::wraps_others) {
        (
            "minimal",
            "it encrypts or wraps other remotes, whose files Full would keep on this disk",
        )
    } else if near == Some(true) {
        ("minimal", "it is on the local network")
    } else {
        ("full", "what is read is read again from this disk")
    }
}

impl MountOptions {
    /// These options as a mount is made with them: Automatic's cache,
    /// `chosen`, where none was chosen.
    #[must_use]
    pub fn mounting(mut self, chosen: &str) -> MountOptions {
        if self.cache.is_none() {
            self.cache = Some(chosen.to_owned());
        }
        self
    }
}

/// Whether the tools that mount `scheme` take options: rclone and sshfs.
/// Never a device's, which GVfs alone mounts.
pub fn takes_options(scheme: Scheme) -> bool {
    !matches!(scheme, Scheme::Nfs | Scheme::Afp) && scheme.device().is_none()
}

/// The options chosen for `top`'s server.
pub fn of(app: &App, top: &Address) -> MountOptions {
    let all: BTreeMap<String, MountOptions> = app.settings.get(SETTING).unwrap_or_default();
    all.get(&top.to_string()).cloned().unwrap_or_default()
}

/// Keep `options` for `top`'s server (the defaults: none kept).
///
/// # Errors
/// When the plugin's data cannot be saved.
pub fn keep(app: &App, top: &Address, options: &MountOptions) -> anyhow::Result<()> {
    let mut all: BTreeMap<String, MountOptions> = app.settings.get(SETTING).unwrap_or_default();
    if *options == MountOptions::default() {
        all.remove(&top.to_string());
    } else {
        all.insert(top.to_string(), options.clone());
    }
    app.settings.set(SETTING, toml::Value::try_from(&all)?);
    app.settings.save()
}

/// Whether `limit` is a bandwidth rclone takes: a rate (`10M`, `512k`,
/// `1.5M`), or an upload's and a download's (`10M:1M`), or `off`.
pub fn bandwidth_refusal(limit: &str) -> Option<&'static str> {
    let rate = |rate: &str| {
        let digits = rate.trim_end_matches(['B', 'K', 'M', 'G', 'T', 'b', 'k', 'm', 'g', 't']);
        !digits.is_empty()
            && digits.chars().all(|c| c.is_ascii_digit() || c == '.')
            && digits.parse::<f64>().is_ok()
    };
    let fine = limit == "off"
        || match limit.split_once(':') {
            Some((up, down)) => rate(up) && rate(down),
            None => rate(limit),
        };
    (!fine).then_some(
        "A bandwidth is a rate, as 10M or 512k, or an upload's and a download's, as 10M:1M",
    )
}

/// The form of `top`'s options, holding `now`; `automatic` is what
/// Automatic chooses there, and why (`automatic`).
pub fn form(top: &Address, now: &MountOptions, automatic: (&str, &str)) -> Form {
    let mut form = Form::new(format!("Mount Options of “{}”", top.label()), "Save")
        .intro("Used when it next connects, by rclone or sshfs. GVfs and KIO mount as the desktop does.")
        .field(
            Field::new("read_only", "Read Only", Kind::Toggle)
                .value(now.read_only.to_string())
                .help("Nothing written there, by any program"),
        );
    if top.scheme == Scheme::Sftp {
        form = form.field(
            Field::new("compression", "Compression", Kind::Toggle)
                .value(now.compression.to_string())
                .help("ssh compresses what it sends (sshfs): faster on a slow network, slower on a fast one"),
        );
    }
    form.field(
        Field::new(
            "cache",
            "File Cache",
            Kind::Choice {
                choices: std::iter::once(
                    Choice::new("", "automatic")
                        .detail(format!("{} here: {}", automatic.0, automatic.1)),
                )
                .chain(
                    CACHES
                        .iter()
                        .map(|(value, detail)| Choice::new(*value, *value).detail(*detail)),
                )
                .collect(),
                open: false,
            },
        )
        .value(now.cache.clone().unwrap_or_default())
        .help("What rclone keeps of its files on this disk"),
    )
    .field(
        Field::text("bandwidth", "Bandwidth Limit")
            .value(now.bandwidth.clone().unwrap_or_default())
            .help("rclone's: 10M, or an upload's and a download's as 10M:1M; empty for none"),
    )
}

/// The options the form's `values` say.
///
/// # Errors
/// What a value cannot be, in the user's words.
pub fn of_values(values: &Values) -> anyhow::Result<MountOptions> {
    let bandwidth = values.get("bandwidth").trim();
    if !bandwidth.is_empty()
        && let Some(refusal) = bandwidth_refusal(bandwidth)
    {
        anyhow::bail!("{refusal}");
    }
    let cache = values.get("cache");
    Ok(MountOptions {
        read_only: values.on("read_only"),
        cache: (!cache.is_empty()).then(|| cache.to_owned()),
        bandwidth: (!bandwidth.is_empty()).then(|| bandwidth.to_owned()),
        compression: values.on("compression"),
    })
}

/// What `rclone mount` is given beside Skwir's own arguments: later on
/// its line, they win over them.
pub fn rclone_arguments(options: &MountOptions) -> Vec<String> {
    let mut args = Vec::new();
    if options.read_only {
        args.push("--read-only".to_owned());
    }
    if let Some(cache) = &options.cache {
        args.extend(["--vfs-cache-mode".to_owned(), cache.clone()]);
    }
    if let Some(bandwidth) = &options.bandwidth {
        args.extend(["--bwlimit".to_owned(), bandwidth.clone()]);
    }
    args
}

/// What sshfs is given beside Skwir's own options, as one `-o`'s.
pub fn sshfs_options(options: &MountOptions) -> Option<String> {
    let mut said = Vec::new();
    if options.read_only {
        said.push("ro");
    }
    if options.compression {
        said.push("compression=yes");
    }
    (!said.is_empty()).then(|| said.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bandwidth_is_one_rclone_takes() {
        for fine in ["10M", "512k", "1.5M", "10M:1M", "off", "100"] {
            assert_eq!(bandwidth_refusal(fine), None, "{fine}");
        }
        for wrong in ["", "fast", "10 M", "-1M", "10M:", ":1M", "1M;rm"] {
            assert!(bandwidth_refusal(wrong).is_some(), "{wrong}");
        }
    }

    #[test]
    fn options_are_given_to_each_tool_as_it_takes_them() {
        let options = MountOptions {
            read_only: true,
            cache: Some("full".into()),
            bandwidth: Some("10M:1M".into()),
            compression: true,
        };
        assert_eq!(
            rclone_arguments(&options),
            [
                "--read-only",
                "--vfs-cache-mode",
                "full",
                "--bwlimit",
                "10M:1M"
            ]
        );
        assert_eq!(
            sshfs_options(&options).as_deref(),
            Some("ro,compression=yes")
        );
        assert!(rclone_arguments(&MountOptions::default()).is_empty());
        assert_eq!(sshfs_options(&MountOptions::default()), None);
    }

    #[test]
    fn automatic_keeps_what_a_cloud_reads_but_not_near_or_wrapped() {
        assert_eq!(automatic(Some("dropbox"), None).0, "full");
        assert_eq!(automatic(None, Some(false)).0, "full");
        assert_eq!(automatic(Some("sftp"), Some(true)).0, "minimal");
        for wraps in ["crypt", "alias", "union", "combine"] {
            assert_eq!(automatic(Some(wraps), None).0, "minimal", "{wraps}");
        }
        // Chosen, it stands; Automatic's where nothing was.
        let chosen = MountOptions {
            cache: Some("minimal".into()),
            ..MountOptions::default()
        };
        assert_eq!(chosen.mounting("full").cache.as_deref(), Some("minimal"));
        let none = MountOptions::default().mounting("full");
        assert_eq!(none.cache.as_deref(), Some("full"));
    }

    #[test]
    fn the_form_says_the_options_back() {
        let top = Address::parse("sftp://me@nas/").unwrap();
        let mut values = Values::default();
        let automatic = automatic(Some("sftp"), Some(true));
        for field in &form(&top, &MountOptions::default(), automatic).fields {
            values.set(field.key.clone(), field.value.clone());
        }
        assert_eq!(of_values(&values).unwrap(), MountOptions::default());
        // Minimal chosen is kept, not taken for Automatic (ADR-102).
        values.set("cache", "minimal");
        assert_eq!(
            of_values(&values).unwrap().cache.as_deref(),
            Some("minimal")
        );
        values.set("read_only", "true");
        values.set("bandwidth", "2M");
        values.set("cache", "writes");
        let chosen = of_values(&values).unwrap();
        assert!(chosen.read_only);
        assert_eq!(chosen.cache.as_deref(), Some("writes"));
        values.set("bandwidth", "fast");
        assert!(of_values(&values).is_err());
        assert!(
            !form(
                &Address::parse("rclone://photos/").unwrap(),
                &MountOptions::default(),
                automatic,
            )
            .fields
            .iter()
            .any(|f| f.key == "compression"),
            "ssh's alone"
        );
    }
}
