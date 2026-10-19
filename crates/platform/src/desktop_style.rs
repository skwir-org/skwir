//! What the user chose on their desktop for how applications look
//! (ADR-071): an accent, a preference for more contrast, the interface
//! font's family. Skwir's own controls wear them; it never imitates a
//! desktop's palette.
//!
//! On Linux the accent and the contrast are the settings portal's
//! (`org.freedesktop.appearance`, over D-Bus). The font is read where
//! the desktop keeps it (`desktop_settings`: `GSettings`, `kdeglobals`,
//! xfconf), then in GTK's `settings.ini`, then from the portal
//! (`org.gnome.desktop.interface`, which it answers outside GNOME too).
//! macOS and Windows say nothing yet: skwir's own colors and font there,
//! until they are read (ADR-071, step 5).

use skwir_core::CancelToken;

/// Whether this system's desktop is read: Linux's; macOS's and Windows'
/// in a later step (ADR-071).
pub const READ: bool = cfg!(target_os = "linux");

/// What the desktop says of how applications look; `Default` when it
/// says nothing.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DesktopStyle {
    /// The accent, as sRGB channels from 0 to 1.
    pub accent: Option<[f32; 3]>,
    /// The desktop asks for more contrast.
    pub more_contrast: bool,
    /// The interface font's family (`Noto Sans`), without its size or
    /// style.
    pub font: Option<String>,
}

/// What the desktop says now. It asks the session's bus and may run a
/// desktop's settings program: on a worker.
pub fn read() -> DesktopStyle {
    imp::read()
}

/// What the desktop says, then what it says each time it changes:
/// `changed` is called from a thread of its own, at once and on each
/// change, until the bus goes or a change arrives after `stop` was
/// cancelled. The thread waits on the bus between changes, never on a
/// timer.
pub fn follow(changed: impl FnMut(DesktopStyle) + Send + 'static, stop: CancelToken) {
    imp::follow(changed, stop);
}

/// The family of a font as Pango describes it (`Noto Sans Bold 10`,
/// `Cantarell, Noto Sans 11`): the first family named, without the size
/// or the style that follow it.
pub fn pango_family(description: &str) -> Option<String> {
    // Pango's words for a style, a weight, a stretch or a variant.
    const STYLE: &[&str] = &[
        "normal",
        "roman",
        "oblique",
        "italic",
        "small-caps",
        "all-small-caps",
        "petite-caps",
        "all-petite-caps",
        "unicase",
        "title-caps",
        "thin",
        "ultra-light",
        "extra-light",
        "light",
        "semi-light",
        "demi-light",
        "book",
        "regular",
        "medium",
        "semi-bold",
        "demi-bold",
        "bold",
        "ultra-bold",
        "extra-bold",
        "heavy",
        "black",
        "ultra-black",
        "extra-black",
        "ultra-heavy",
        "extra-heavy",
        "ultra-condensed",
        "extra-condensed",
        "condensed",
        "semi-condensed",
        "semi-expanded",
        "expanded",
        "extra-expanded",
        "ultra-expanded",
    ];
    let mut words: Vec<&str> = description.split_whitespace().collect();
    if words
        .last()
        .is_some_and(|w| w.trim_end_matches("px").parse::<f32>().is_ok())
    {
        words.pop();
    }
    while words
        .last()
        .is_some_and(|w| STYLE.contains(&w.to_ascii_lowercase().as_str()))
    {
        words.pop();
    }
    let families = words.join(" ");
    let first = families.split(',').next()?.trim();
    (!first.is_empty()).then(|| first.to_owned())
}

/// The family of a font as Qt writes it in `kdeglobals` (`Noto
/// Sans,10,-1,5,400,0,0,0,0,0`).
pub fn qt_family(font: &str) -> Option<String> {
    let family = font.split(',').next()?.trim();
    (!family.is_empty()).then(|| family.to_owned())
}

#[cfg(target_os = "linux")]
mod imp {
    use skwir_core::CancelToken;
    use zbus::blocking::Connection;
    use zbus::zvariant::{OwnedValue, Value};

    use super::DesktopStyle;
    use crate::desktop_settings::{self as desktop, Chosen};

    const DESTINATION: &str = "org.freedesktop.portal.Desktop";
    const PATH: &str = "/org/freedesktop/portal/desktop";
    const SETTINGS: &str = "org.freedesktop.portal.Settings";
    const APPEARANCE: &str = "org.freedesktop.appearance";
    const INTERFACE: &str = "org.gnome.desktop.interface";

    pub(super) fn read() -> DesktopStyle {
        let bus = Connection::session().ok();
        read_from(bus.as_ref(), own_font())
    }

    pub(super) fn follow(
        mut changed: impl FnMut(DesktopStyle) + Send + 'static,
        stop: CancelToken,
    ) {
        let thread = std::thread::Builder::new()
            .name("skwir-desktop-style".into())
            .spawn(move || {
                let bus = Connection::session();
                if let Err(error) = &bus {
                    tracing::debug!("the desktop's style is not followed: {error}");
                }
                follow_on(bus.as_ref().ok(), own_font, &mut changed, &stop);
            });
        if let Err(error) = thread {
            tracing::debug!("the desktop's style is not followed: {error}");
        }
    }

    /// What the desktop says, its font as `own_font` found it where the
    /// desktop keeps it, else as the portal on `bus` names it.
    pub(super) fn read_from(bus: Option<&Connection>, own_font: Option<String>) -> DesktopStyle {
        let portal =
            |namespace: &str, key: &str| bus.and_then(|bus| portal_read(bus, namespace, key));
        DesktopStyle {
            accent: portal(APPEARANCE, "accent-color").and_then(accent),
            more_contrast: portal(APPEARANCE, "contrast")
                .and_then(|value| u32::try_from(value).ok())
                == Some(1),
            font: own_font.or_else(|| {
                portal(INTERFACE, "font-name")
                    .and_then(|value| String::try_from(value).ok())
                    .and_then(|name| super::pango_family(&name))
            }),
        }
    }

    /// Read on `bus`, tell `changed`, then again at each change of what
    /// is read, until the bus goes or a change comes after `stop`.
    pub(super) fn follow_on(
        bus: Option<&Connection>,
        own_font: impl Fn() -> Option<String>,
        changed: &mut dyn FnMut(DesktopStyle),
        stop: &CancelToken,
    ) {
        let mut said = read_from(bus, own_font());
        changed(said.clone());
        let Some(bus) = bus else { return };
        let rule = zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .interface(SETTINGS)
            .and_then(|rule| rule.member("SettingChanged"))
            .map(zbus::match_rule::Builder::build);
        let messages =
            rule.and_then(|rule| zbus::blocking::MessageIterator::for_match_rule(rule, bus, None));
        let messages = match messages {
            Ok(messages) => messages,
            Err(error) => {
                tracing::debug!("the desktop's style is read, not followed: {error}");
                return;
            }
        };
        for message in messages {
            if stop.is_cancelled() {
                break;
            }
            let Ok(message) = message else { continue };
            let Ok((namespace, key, _)) =
                message.body().deserialize::<(String, String, OwnedValue)>()
            else {
                continue;
            };
            let read = match key.as_str() {
                "accent-color" | "contrast" => namespace == APPEARANCE,
                "font-name" => namespace.ends_with(".interface"),
                _ => false,
            };
            if !read {
                continue;
            }
            let now = read_from(Some(bus), own_font());
            if now != said {
                said = now.clone();
                changed(now);
            }
        }
    }

    /// A setting the portal on `bus` answers: `ReadOne`, or `Read` for a
    /// portal before its second version, which wraps the value once more.
    fn portal_read(bus: &Connection, namespace: &str, key: &str) -> Option<OwnedValue> {
        let call = |method: &str| {
            bus.call_method(
                Some(DESTINATION),
                PATH,
                Some(SETTINGS),
                method,
                &(namespace, key),
            )
            .ok()
        };
        if let Some(reply) = call("ReadOne") {
            return reply.body().deserialize::<OwnedValue>().ok();
        }
        let value = call("Read")?.body().deserialize::<OwnedValue>().ok()?;
        match &*value {
            Value::Value(inner) => inner.try_to_owned().ok(),
            _ => Some(value),
        }
    }

    /// An accent as the portal says it: three channels from 0 to 1, any
    /// outside them meaning none chosen.
    fn accent(value: OwnedValue) -> Option<[f32; 3]> {
        let (r, g, b) = <(f64, f64, f64)>::try_from(value).ok()?;
        let channels = [r, g, b];
        channels
            .iter()
            .all(|c| (0. ..=1.).contains(c))
            .then(|| channels.map(|c| c as f32))
    }

    /// The font the desktop's own settings name, else GTK's `settings.ini`.
    fn own_font() -> Option<String> {
        let own = match desktop::chosen() {
            Some(Chosen::GSettings(schema)) => desktop::gsettings(schema, "font-name")
                .as_deref()
                .and_then(super::pango_family),
            Some(Chosen::Kde) => desktop::kdeglobals()
                .and_then(|text| desktop::keyfile_value(&text, "General", "font"))
                .as_deref()
                .and_then(super::qt_family),
            Some(Chosen::Xfce) => desktop::xfconf("/Gtk/FontName")
                .as_deref()
                .and_then(super::pango_family),
            None => None,
        };
        own.or_else(|| {
            desktop::gtk_settings("gtk-font-name")
                .first()
                .and_then(|name| super::pango_family(name))
        })
    }
}

// macOS and Windows: their accent and contrast are read in a later step
// (ADR-071, step 5); until then the desktop says nothing, and skwir wears
// its own colors and font.
#[cfg(not(target_os = "linux"))]
mod imp {
    use skwir_core::CancelToken;

    use super::DesktopStyle;

    pub(super) fn read() -> DesktopStyle {
        DesktopStyle::default()
    }

    pub(super) fn follow(mut changed: impl FnMut(DesktopStyle) + Send + 'static, _: CancelToken) {
        changed(DesktopStyle::default());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pango_font_s_family_is_its_name_without_size_or_style() {
        let family = |description| pango_family(description);
        assert_eq!(family("Noto Sans  10").as_deref(), Some("Noto Sans"));
        assert_eq!(family("Cantarell 11").as_deref(), Some("Cantarell"));
        assert_eq!(family("Noto Sans Bold 10").as_deref(), Some("Noto Sans"));
        assert_eq!(family("Sans Bold Italic 12").as_deref(), Some("Sans"));
        assert_eq!(
            family("DejaVu Sans Book 10").as_deref(),
            Some("DejaVu Sans")
        );
        assert_eq!(
            family("Cantarell, Noto Sans 11").as_deref(),
            Some("Cantarell")
        );
        assert_eq!(
            family("Inter Variable 11").as_deref(),
            Some("Inter Variable")
        );
        assert_eq!(family("Monospace 13px").as_deref(), Some("Monospace"));
        assert_eq!(family("Adwaita Sans").as_deref(), Some("Adwaita Sans"));
        assert_eq!(family("12"), None);
        assert_eq!(family(""), None);
    }

    #[test]
    fn a_qt_font_s_family_is_what_comes_before_its_first_comma() {
        assert_eq!(
            qt_family("Noto Sans,10,-1,5,400,0,0,0,0,0").as_deref(),
            Some("Noto Sans")
        );
        assert_eq!(qt_family("Hack"), Some("Hack".to_owned()));
        assert_eq!(qt_family(",10"), None);
    }

    #[cfg(target_os = "linux")]
    mod portal {
        use std::collections::HashMap;
        use std::sync::{Arc, Mutex, mpsc};
        use std::time::Duration;

        use skwir_core::CancelToken;
        use zbus::blocking::Connection;
        use zbus::object_server::SignalEmitter;
        use zbus::zvariant::{OwnedValue, Value};

        use super::super::{DesktopStyle, imp};

        type Values = Arc<Mutex<HashMap<(String, String), OwnedValue>>>;

        /// A settings portal that answers what `values` holds.
        struct Portal {
            values: Values,
        }

        #[zbus::interface(name = "org.freedesktop.portal.Settings")]
        impl Portal {
            fn read_one(&self, namespace: String, key: String) -> zbus::fdo::Result<OwnedValue> {
                let values = self.values.lock().unwrap();
                let value = values.get(&(namespace, key));
                let value =
                    value.ok_or_else(|| zbus::fdo::Error::Failed("no such setting".into()))?;
                value
                    .try_clone()
                    .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
            }

            #[zbus(signal)]
            async fn setting_changed(
                emitter: &SignalEmitter<'_>,
                namespace: &str,
                key: &str,
                value: Value<'_>,
            ) -> zbus::Result<()>;
        }

        /// A connection to a portal of the test's own, and the portal's
        /// end, over a pair of sockets: no bus, never the session's.
        /// A portal before its second version: `Read` alone, the value in
        /// a variant of its own.
        struct OldPortal {
            values: Values,
        }

        #[zbus::interface(name = "org.freedesktop.portal.Settings")]
        impl OldPortal {
            fn read(&self, namespace: String, key: String) -> zbus::fdo::Result<OwnedValue> {
                let values = self.values.lock().unwrap();
                let value = values.get(&(namespace, key));
                let value =
                    value.ok_or_else(|| zbus::fdo::Error::Failed("no such setting".into()))?;
                let failed = |e: zbus::zvariant::Error| zbus::fdo::Error::Failed(e.to_string());
                let inner = Value::from(value.try_clone().map_err(failed)?);
                OwnedValue::try_from(Value::Value(Box::new(inner))).map_err(failed)
            }
        }

        fn portal(values: &Values) -> (Connection, Connection) {
            serve(Portal {
                values: Arc::clone(values),
            })
        }

        fn serve(answering: impl zbus::object_server::Interface) -> (Connection, Connection) {
            let (theirs, ours) = std::os::unix::net::UnixStream::pair().unwrap();
            let server = std::thread::spawn(move || {
                zbus::blocking::connection::Builder::async_io_unix_stream(theirs)
                    .server(zbus::Guid::generate())
                    .unwrap()
                    .p2p()
                    .serve_at("/org/freedesktop/portal/desktop", answering)
                    .unwrap()
                    .build()
                    .unwrap()
            });
            let client = zbus::blocking::connection::Builder::async_io_unix_stream(ours)
                .p2p()
                .build()
                .unwrap();
            (client, server.join().unwrap())
        }

        fn set(values: &Values, namespace: &str, key: &str, value: &Value<'_>) {
            values.lock().unwrap().insert(
                (namespace.to_owned(), key.to_owned()),
                value.try_to_owned().unwrap(),
            );
        }

        fn say(server: &Connection, namespace: &str, key: &str, value: Value<'_>) {
            let emitter =
                SignalEmitter::new(server.inner(), "/org/freedesktop/portal/desktop").unwrap();
            zbus::block_on(Portal::setting_changed(&emitter, namespace, key, value)).unwrap();
        }

        const APPEARANCE: &str = "org.freedesktop.appearance";

        #[test]
        fn what_the_portal_answers_is_the_desktop_s_style() {
            let values = Values::default();
            set(
                &values,
                APPEARANCE,
                "accent-color",
                &Value::from((0.2_f64, 0.4, 0.6)),
            );
            set(&values, APPEARANCE, "contrast", &Value::from(1_u32));
            set(
                &values,
                "org.gnome.desktop.interface",
                "font-name",
                &Value::from("Noto Sans 10"),
            );
            let (bus, _server) = portal(&values);
            let style = imp::read_from(Some(&bus), None);
            assert_eq!(
                style,
                DesktopStyle {
                    accent: Some([0.2, 0.4, 0.6]),
                    more_contrast: true,
                    font: Some("Noto Sans".into()),
                }
            );
            // The desktop's own settings name the font before the portal.
            let own = imp::read_from(Some(&bus), Some("Cantarell".into()));
            assert_eq!(own.font.as_deref(), Some("Cantarell"));
        }

        #[test]
        fn a_portal_before_read_one_is_read_too() {
            let values = Values::default();
            set(
                &values,
                APPEARANCE,
                "accent-color",
                &Value::from((0.2_f64, 0.4, 0.6)),
            );
            set(&values, APPEARANCE, "contrast", &Value::from(1_u32));
            let (bus, _server) = serve(OldPortal {
                values: Arc::clone(&values),
            });
            let style = imp::read_from(Some(&bus), None);
            assert_eq!(style.accent, Some([0.2, 0.4, 0.6]));
            assert!(style.more_contrast);
        }

        #[test]
        fn an_accent_out_of_range_or_a_setting_not_answered_is_none() {
            let values = Values::default();
            set(
                &values,
                APPEARANCE,
                "accent-color",
                &Value::from((-1_f64, -1., -1.)),
            );
            let (bus, _server) = portal(&values);
            assert_eq!(imp::read_from(Some(&bus), None), DesktopStyle::default());
            // No bus at all: skwir's own.
            assert_eq!(imp::read_from(None, None), DesktopStyle::default());
        }

        #[test]
        fn a_change_the_portal_says_is_followed_and_only_one_that_changes_something() {
            let values = Values::default();
            set(
                &values,
                APPEARANCE,
                "accent-color",
                &Value::from((0.2_f64, 0.4, 0.6)),
            );
            let (bus, server) = portal(&values);
            let (sent, got) = mpsc::channel();
            let stop = CancelToken::new();
            let following = std::thread::spawn(move || {
                imp::follow_on(
                    Some(&bus),
                    || None,
                    &mut |style| sent.send(style).unwrap(),
                    &stop,
                );
            });
            let next = || got.recv_timeout(Duration::from_secs(10)).unwrap();
            assert_eq!(next().accent, Some([0.2, 0.4, 0.6]));

            // A setting it does not read, and one that says what it said:
            // nothing. Then the accent and the contrast change.
            say(&server, APPEARANCE, "color-scheme", Value::from(1_u32));
            say(
                &server,
                APPEARANCE,
                "accent-color",
                Value::from((0.2_f64, 0.4, 0.6)),
            );
            assert!(got.recv_timeout(Duration::from_millis(300)).is_err());
            set(
                &values,
                APPEARANCE,
                "accent-color",
                &Value::from((0.9_f64, 0.5, 0.1)),
            );
            set(&values, APPEARANCE, "contrast", &Value::from(1_u32));
            say(&server, APPEARANCE, "contrast", Value::from(1_u32));
            let changed = next();
            assert_eq!(changed.accent, Some([0.9, 0.5, 0.1]));
            assert!(changed.more_contrast);
            assert!(got.recv_timeout(Duration::from_millis(300)).is_err());

            // The portal gone, the thread ends.
            drop(server);
            following.join().unwrap();
        }
    }
}
