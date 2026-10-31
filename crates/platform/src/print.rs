//! Printing through the system's own dialog (ADR-111): ask it what and
//! where to print (`ask`), then hand it the PDF made for that (`send`).
//! On Linux the desktop's print portal shows the desktop's dialog; on
//! macOS the print panel chooses and CUPS prints; on Windows the print
//! dialog chooses and the PDF helper draws the pages on the printer.
//!
//! The dialog chooses; Skwir applies: a portal prints the file as it is
//! handed, the printer making only the copies and the pages per sheet, so
//! the pages chosen, even or odd sides, the reverse order and the scale
//! are Skwir's to apply to that file (as GTK's own printing through the
//! portal does).
use skwir_core::CancelToken;
use skwir_core::print::{Duplex, Margins, POINTS_PER_MM, PageSet, Pages, Paper};
use std::path::{Path, PathBuf};

/// What a dialog is asked.
#[derive(Clone, Debug, Default)]
pub struct Ask {
    /// The document's name, the dialog's title.
    pub title: String,
    /// Whether the preview shows a page of it: "Current page" is offered.
    pub current_page: bool,
    /// Whether something of it is selected: "Selection" is offered.
    pub selection: bool,
}

/// What the dialog chose, and what it keeps to print with.
#[derive(Clone, Debug)]
pub struct Chosen {
    pub pages: Pages,
    pub page_set: PageSet,
    /// Last page first.
    pub reverse: bool,
    /// 1 is as it is.
    pub scale: f32,
    /// The sheet, turned as chosen, its margins at least the printer's.
    pub paper: Paper,
    pub duplex: Duplex,
    /// The printer's name, to say where it went.
    pub printer: Option<String>,
    /// The file the dialog prints to, when it prints to one.
    pub to_file: Option<PathBuf>,
    system: System,
}

/// What a system's dialog keeps between choosing and printing.
#[derive(Clone, Debug)]
enum System {
    /// The token the portal gave with the dialog's settings.
    #[cfg(target_os = "linux")]
    Portal(u32),
    /// The printer, and what CUPS is told beside the file.
    #[cfg(target_os = "macos")]
    Cups { printer: String, copies: u32 },
    /// The printer, and its settings as the dialog left them (`DEVMODEW`).
    #[cfg(windows)]
    Printer { name: String, devmode: Vec<u8> },
    /// Nothing: no dialog on this system.
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    None,
}

/// The margins Skwir lays text out in, at least: 15 mm.
const MARGIN: f32 = 15.0 * POINTS_PER_MM;

/// The paper this system and its user write on, when no dialog says:
/// `$PAPERSIZE`, then libpaper's `/etc/papersize`, then the region of
/// the locale (`LC_PAPER`, `LC_ALL`, `LANG`): Letter in the Americas
/// that use it and the Philippines, A4 elsewhere.
pub fn paper() -> Paper {
    let named = std::env::var("PAPERSIZE").ok().or_else(|| {
        #[cfg(not(windows))]
        return std::fs::read_to_string("/etc/papersize").ok();
        #[cfg(windows)]
        return None;
    });
    if let Some(paper) = named.as_deref().and_then(paper_named) {
        return paper;
    }
    let locale = ["LC_PAPER", "LC_ALL", "LANG"]
        .iter()
        .find_map(|name| std::env::var(name).ok().filter(|value| !value.is_empty()))
        .unwrap_or_default();
    paper_of_locale(&locale)
}

/// libpaper's names that matter: the first word not a comment.
fn paper_named(text: &str) -> Option<Paper> {
    let name = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))?
        .to_lowercase();
    match name.as_str() {
        "a4" => Some(Paper::a4()),
        "letter" => Some(Paper::letter()),
        "legal" => Some(Paper::mm(215.9, 355.6)),
        "a3" => Some(Paper::mm(297.0, 420.0)),
        "a5" => Some(Paper::mm(148.0, 210.0)),
        _ => None,
    }
}

/// A4, or Letter where the locale's region writes on Letter.
fn paper_of_locale(locale: &str) -> Paper {
    const LETTER: [&str; 7] = ["US", "CA", "MX", "CL", "CO", "VE", "PH"];
    let region = locale
        .split(['.', '@'])
        .next()
        .and_then(|name| name.split('_').nth(1))
        .unwrap_or_default();
    if LETTER.contains(&region) {
        Paper::letter()
    } else {
        Paper::a4()
    }
}

/// Ask the system's dialog what and where to print: what it chose, or
/// `None` when it was cancelled. It waits for the user: a worker's work.
///
/// # Errors
/// When this system has no print dialog Skwir can show (a desktop
/// without a print portal), or the dialog failed.
pub fn ask(ask: &Ask, cancel: &CancelToken) -> anyhow::Result<Option<Chosen>> {
    #[cfg(target_os = "linux")]
    {
        let bus = portal::connection()?;
        portal::ask(&bus, Some(portal::DESTINATION), ask, cancel)
    }
    #[cfg(target_os = "macos")]
    {
        macos::ask(ask, cancel)
    }
    #[cfg(windows)]
    {
        windows::ask(ask, cancel)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = (ask, cancel);
        Err(skwir_core::system::not_yet("Printing").into())
    }
}

/// Print `pdf` as `chosen` says, with `title` as the job's name: handed to
/// the system's printing. The pages, sides and order were applied to it.
/// It may wait for the system: a worker's work.
///
/// # Errors
/// When the system refused it.
pub fn send(chosen: &Chosen, title: &str, pdf: &Path, cancel: &CancelToken) -> anyhow::Result<()> {
    #[cfg(target_os = "linux")]
    {
        let bus = portal::connection()?;
        portal::send(&bus, Some(portal::DESTINATION), chosen, title, pdf, cancel)
    }
    #[cfg(target_os = "macos")]
    {
        macos::send(chosen, title, pdf, cancel)
    }
    #[cfg(windows)]
    {
        windows::send(chosen, title, pdf, cancel)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = (chosen, title, pdf, cancel);
        Err(skwir_core::system::not_yet("Printing").into())
    }
}

/// The sheet as a dialog gives it, in millimetres, its printable margins
/// widened to `MARGIN`.
fn sheet(width_mm: f32, height_mm: f32, landscape: bool, margins_mm: [f32; 4]) -> Paper {
    let [top, right, bottom, left] = margins_mm.map(|mm| (mm * POINTS_PER_MM).max(MARGIN));
    let paper = Paper {
        width: width_mm * POINTS_PER_MM,
        height: height_mm * POINTS_PER_MM,
        margins: Margins {
            top,
            right,
            bottom,
            left,
        },
    };
    if landscape { paper.landscape() } else { paper }
}

/// The print portal (`org.freedesktop.portal.Print`): `PreparePrint`
/// shows the desktop's dialog and answers its settings and a token,
/// `Print` prints a file handed by descriptor with them.
#[cfg(target_os = "linux")]
mod portal {
    use super::{Ask, Chosen, Duplex, PageSet, Pages, System, sheet};
    use crossbeam_channel::bounded;
    use skwir_core::CancelToken;
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::atomic::{AtomicU32, Ordering};
    use zbus::blocking::Connection;
    use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

    pub(super) const DESTINATION: &str = "org.freedesktop.portal.Desktop";
    /// How long the portal may take to answer a call (not the dialog: its
    /// answer comes later, as a signal): one that hangs is given up on.
    const CALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

    /// A session connection of its own, with a call timeout, for the time
    /// a print takes: the shared one has none, and a portal that hangs
    /// would hold its call for good (AGENTS.md, the session bus).
    pub(super) fn connection() -> zbus::Result<Connection> {
        zbus::blocking::connection::Builder::session()?
            .method_timeout(CALL_TIMEOUT)
            .build()
    }
    const PATH: &str = "/org/freedesktop/portal/desktop";
    const PRINT: &str = "org.freedesktop.portal.Print";
    const REQUEST: &str = "org.freedesktop.portal.Request";

    type Results = HashMap<String, OwnedValue>;

    /// `PreparePrint`: the dialog, and what it chose.
    pub(super) fn ask(
        bus: &Connection,
        destination: Option<&str>,
        ask: &Ask,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<Chosen>> {
        let empty: HashMap<&str, Value<'_>> = HashMap::new();
        let Some(results) = request(bus, destination, cancel, |token| {
            let mut options: HashMap<&str, Value<'_>> = HashMap::new();
            options.insert("handle_token", Value::from(token));
            options.insert("modal", Value::from(true));
            options.insert("accept_label", Value::from("Print"));
            options.insert("has_current_page", Value::from(ask.current_page));
            options.insert("has_selected_pages", Value::from(ask.selection));
            options.insert("supported_output_file_formats", Value::from(vec!["pdf"]));
            bus.call_method(
                destination,
                PATH,
                Some(PRINT),
                "PreparePrint",
                &("", ask.title.as_str(), &empty, &empty, options),
            )
        })?
        else {
            return Ok(None);
        };
        Ok(Some(chosen(&results)?))
    }

    /// What the portal's answer says, read as `Chosen`.
    pub(super) fn chosen(results: &Results) -> anyhow::Result<Chosen> {
        let token: u32 = results
            .get("token")
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| anyhow::anyhow!("the print dialog answered no token"))?;
        let dict = |key: &str| -> HashMap<String, OwnedValue> {
            results
                .get(key)
                .and_then(|v| v.try_clone().ok())
                .and_then(|v| HashMap::<String, OwnedValue>::try_from(v).ok())
                .unwrap_or_default()
        };
        let settings = dict("settings");
        let setup = dict("page-setup");
        let text = |map: &HashMap<String, OwnedValue>, key: &str| -> Option<String> {
            map.get(key)
                .and_then(|v| String::try_from(v.try_clone().ok()?).ok())
        };
        let number = |key: &str| -> Option<f32> {
            setup
                .get(key)
                .and_then(|v| f64::try_from(v).ok())
                .map(|n| n as f32)
                .filter(|n| n.is_finite() && *n >= 0.0)
        };
        let pages = match text(&settings, "print-pages").as_deref() {
            Some("current") => Pages::Current,
            Some("selection") => Pages::Selection,
            Some("ranges") => text(&settings, "page-ranges")
                .and_then(|ranges| skwir_core::print::from_zero_based(&ranges).ok())
                .map_or(Pages::All, Pages::Ranges),
            _ => Pages::All,
        };
        let page_set = match text(&settings, "page-set").as_deref() {
            Some("even") => PageSet::Even,
            Some("odd") => PageSet::Odd,
            _ => PageSet::All,
        };
        // GTK's: horizontal turns over the long edge, vertical the short.
        let duplex = match text(&settings, "duplex").as_deref() {
            Some("horizontal") => Duplex::LongEdge,
            Some("vertical") => Duplex::ShortEdge,
            _ => Duplex::Simplex,
        };
        let landscape = text(&setup, "Orientation")
            .or_else(|| text(&settings, "orientation"))
            .is_some_and(|orientation| orientation.contains("landscape"));
        let fallback = super::paper();
        let paper = match (number("Width"), number("Height")) {
            (Some(width), Some(height)) if width > 0.0 && height > 0.0 => sheet(
                width,
                height,
                landscape,
                ["MarginTop", "MarginRight", "MarginBottom", "MarginLeft"]
                    .map(|key| number(key).unwrap_or(0.0)),
            ),
            _ if landscape => fallback.landscape(),
            _ => fallback,
        };
        Ok(Chosen {
            pages,
            page_set,
            reverse: text(&settings, "reverse").as_deref() == Some("true"),
            scale: text(&settings, "scale")
                .and_then(|scale| scale.parse::<f32>().ok())
                .filter(|scale| scale.is_finite() && *scale > 0.0)
                .map_or(1.0, |scale| scale / 100.0),
            paper,
            duplex,
            printer: text(&settings, "printer"),
            to_file: text(&settings, "output-uri").and_then(|uri| crate::uri::local_path_of(&uri)),
            system: System::Portal(token),
        })
    }

    /// `Print`: `pdf` handed by descriptor, with the token the dialog gave.
    pub(super) fn send(
        bus: &Connection,
        destination: Option<&str>,
        chosen: &Chosen,
        title: &str,
        pdf: &Path,
        cancel: &CancelToken,
    ) -> anyhow::Result<()> {
        // On Linux what the dialog keeps is the portal's token alone.
        let System::Portal(token) = chosen.system;
        let file = crate::file::open_regular(pdf)?;
        let fd = zbus::zvariant::Fd::from(std::os::fd::AsFd::as_fd(&file));
        let sent = request(bus, destination, cancel, |handle| {
            let mut options: HashMap<&str, Value<'_>> = HashMap::new();
            options.insert("handle_token", Value::from(handle));
            options.insert("token", Value::from(token));
            options.insert("modal", Value::from(true));
            bus.call_method(
                destination,
                PATH,
                Some(PRINT),
                "Print",
                &("", title, &fd, options),
            )
        })?;
        anyhow::ensure!(sent.is_some(), "printing was cancelled");
        Ok(())
    }

    /// A portal request: `call` made with a token of its own, then its
    /// `Response` waited for; its results when it answered 0, `None` when
    /// the user cancelled or `cancel` did (the request then closed).
    fn request(
        bus: &Connection,
        destination: Option<&str>,
        cancel: &CancelToken,
        call: impl FnOnce(&str) -> zbus::Result<zbus::Message>,
    ) -> anyhow::Result<Option<Results>> {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let token = format!("skwir_print_{}", NEXT.fetch_add(1, Ordering::Relaxed));
        // The answer comes on the request's own object, whose path the
        // token and the connection's name say: listened for before the
        // call, so a dialog answering at once is not missed. A connection
        // of its own to a portal (a test's) has no name: any answer is it.
        let expected = bus.unique_name().map(|name| {
            let sender = name.as_str().trim_start_matches(':').replace('.', "_");
            format!("{PATH}/request/{sender}/{token}")
        });
        let rule = zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .interface(REQUEST)?
            .member("Response")?
            .build();
        let (ready_tx, ready_rx) = bounded::<()>(1);
        let (told_tx, told_rx) = bounded::<(u32, Results)>(1);
        let stop = cancel.child();
        let listening = {
            let (bus, stop, expected) = (bus.clone(), stop.clone(), expected.clone());
            std::thread::Builder::new()
                .name("skwir-print-portal".into())
                .spawn(move || {
                    crate::bus::listen(
                        &bus,
                        vec![rule],
                        &stop,
                        || {
                            let _ = ready_tx.send(());
                        },
                        |message| {
                            let header = message.header();
                            let ours = expected.as_deref().is_none_or(|expected| {
                                header.path().is_some_and(|path| path.as_str() == expected)
                            });
                            if ours
                                && let Ok((code, results)) =
                                    message.body().deserialize::<(u32, Results)>()
                            {
                                let _ = told_tx.try_send((code, results));
                            }
                        },
                    );
                })?
        };
        let finish = |result| {
            stop.cancel();
            let _ = listening.join();
            result
        };
        if ready_rx.recv().is_err() {
            return finish(Err(anyhow::anyhow!(
                "the print portal cannot be listened to"
            )));
        }
        let handle: OwnedObjectPath =
            match call(&token).and_then(|reply| reply.body().deserialize()) {
                Ok(handle) => handle,
                Err(error) => {
                    return finish(Err(anyhow::anyhow!(
                        "this desktop shows no print dialog (its print portal: {error})"
                    )));
                }
            };
        let cancelled = cancel.subscribe();
        crossbeam_channel::select! {
            recv(told_rx) -> told => match told {
                Ok((0, results)) => finish(Ok(Some(results))),
                // 1: the user cancelled; 2: the dialog ended otherwise.
                Ok((1, _)) => finish(Ok(None)),
                Ok((code, _)) => finish(Err(anyhow::anyhow!("the print dialog ended ({code})"))),
                Err(_) => finish(Err(anyhow::anyhow!("the print portal stopped answering"))),
            },
            recv(cancelled.receiver()) -> _ => {
                let _ = bus.call_method(destination, handle.as_str(), Some(REQUEST), "Close", &());
                finish(Ok(None))
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use skwir_core::print::Range;
        use std::sync::{Arc, Mutex};
        use zbus::object_server::SignalEmitter;

        /// What a stand-in portal was asked and handed.
        #[derive(Default)]
        struct Seen {
            titles: Vec<String>,
            options: Vec<HashMap<String, OwnedValue>>,
            printed: Vec<Vec<u8>>,
        }

        /// A print portal that answers as a desktop's would, the dialog's
        /// choice being `answer` (or cancelled when `None`).
        struct Portal {
            seen: Arc<Mutex<Seen>>,
            answer: Option<HashMap<String, OwnedValue>>,
        }

        const HANDLE: &str = "/org/freedesktop/portal/desktop/request/test/1";

        #[zbus::interface(name = "org.freedesktop.portal.Print")]
        #[allow(clippy::needless_pass_by_value)] // zbus hands its arguments by value.
        impl Portal {
            fn prepare_print(
                &self,
                #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
                parent: String,
                title: String,
                settings: HashMap<String, OwnedValue>,
                setup: HashMap<String, OwnedValue>,
                options: HashMap<String, OwnedValue>,
            ) -> OwnedObjectPath {
                // No window to be modal to; no hints asked.
                let _ = (parent, settings, setup);
                {
                    let mut seen = self.seen.lock().unwrap();
                    seen.titles.push(title);
                    seen.options.push(options);
                }
                let (code, results) = match &self.answer {
                    Some(results) => (
                        0u32,
                        results
                            .iter()
                            .map(|(k, v)| (k.clone(), v.try_clone().unwrap()))
                            .collect(),
                    ),
                    None => (1u32, HashMap::new()),
                };
                respond(&emitter, code, results);
                OwnedObjectPath::try_from(HANDLE).unwrap()
            }

            fn print(
                &self,
                #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
                parent: String,
                title: String,
                fd: zbus::zvariant::OwnedFd,
                options: HashMap<String, OwnedValue>,
            ) -> OwnedObjectPath {
                use std::io::Read as _;
                let _ = parent;
                let mut bytes = Vec::new();
                std::fs::File::from(std::os::fd::OwnedFd::from(fd))
                    .read_to_end(&mut bytes)
                    .unwrap();
                {
                    let mut seen = self.seen.lock().unwrap();
                    seen.titles.push(title);
                    seen.options.push(options);
                    seen.printed.push(bytes);
                }
                respond(&emitter, 0, HashMap::new());
                OwnedObjectPath::try_from(HANDLE).unwrap()
            }
        }

        /// The `Response` of the request, a moment after the method's
        /// reply, as a portal sends it once the dialog closes.
        fn respond(emitter: &SignalEmitter<'_>, code: u32, results: HashMap<String, OwnedValue>) {
            let connection = emitter.connection().clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(50));
                zbus::block_on(connection.emit_signal(
                    None::<&str>,
                    HANDLE,
                    REQUEST,
                    "Response",
                    &(code, results),
                ))
                .unwrap();
            });
        }

        fn serve(portal: Portal) -> (Connection, Connection) {
            let (theirs, ours) = std::os::unix::net::UnixStream::pair().unwrap();
            let server = std::thread::spawn(move || {
                zbus::blocking::connection::Builder::async_io_unix_stream(theirs)
                    .server(zbus::Guid::generate())
                    .unwrap()
                    .p2p()
                    .serve_at(PATH, portal)
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

        fn value(v: impl Into<Value<'static>>) -> OwnedValue {
            OwnedValue::try_from(v.into()).unwrap()
        }

        fn dict(pairs: &[(&str, OwnedValue)]) -> OwnedValue {
            let map: HashMap<String, OwnedValue> = pairs
                .iter()
                .map(|(k, v)| ((*k).to_owned(), v.try_clone().unwrap()))
                .collect();
            OwnedValue::try_from(Value::from(map)).unwrap()
        }

        /// A desktop's answer: A5 landscape, pages 2 to 3 and 5, odd sides,
        /// reversed, turned over the short edge.
        fn answer() -> HashMap<String, OwnedValue> {
            HashMap::from([
                ("token".to_owned(), value(7u32)),
                (
                    "settings".to_owned(),
                    dict(&[
                        ("print-pages", value("ranges")),
                        ("page-ranges", value("1-2,4")),
                        ("page-set", value("odd")),
                        ("reverse", value("true")),
                        ("scale", value("90")),
                        ("duplex", value("vertical")),
                        ("printer", value("Office")),
                    ]),
                ),
                (
                    "page-setup".to_owned(),
                    dict(&[
                        ("Width", value(148.0f64)),
                        ("Height", value(210.0f64)),
                        ("MarginTop", value(4.0f64)),
                        ("MarginLeft", value(30.0f64)),
                        ("Orientation", value("landscape")),
                    ]),
                ),
            ])
        }

        #[test]
        fn the_dialog_s_choice_is_read_and_its_file_printed_with_its_token() {
            let seen = Arc::new(Mutex::new(Seen::default()));
            let (bus, _portal) = serve(Portal {
                seen: Arc::clone(&seen),
                answer: Some(answer()),
            });
            let cancel = CancelToken::new();
            let chosen = super::ask(
                &bus,
                None,
                &Ask {
                    title: "Report.pdf".into(),
                    current_page: true,
                    selection: false,
                },
                &cancel,
            )
            .unwrap()
            .unwrap();
            assert_eq!(
                chosen.pages,
                Pages::Ranges(vec![
                    Range {
                        first: 2,
                        last: Some(3)
                    },
                    Range {
                        first: 5,
                        last: Some(5)
                    }
                ])
            );
            assert_eq!(chosen.page_set, PageSet::Odd);
            assert!(chosen.reverse);
            assert!((chosen.scale - 0.9).abs() < 1e-6);
            assert_eq!(chosen.duplex, Duplex::ShortEdge);
            assert_eq!(chosen.printer.as_deref(), Some("Office"));
            // A5 turned: 210 mm wide; the printer's 30 mm kept, its 4 mm
            // widened to Skwir's 15.
            assert!((chosen.paper.width - 210.0 * skwir_core::print::POINTS_PER_MM).abs() < 0.1);
            assert!(chosen.paper.width > chosen.paper.height);
            let fifteen = 15.0 * skwir_core::print::POINTS_PER_MM;
            let m = chosen.paper.margins;
            let margins = [m.top, m.right, m.bottom, m.left];
            assert!(margins.iter().all(|m| *m >= fifteen - 0.01));
            assert!(
                margins
                    .iter()
                    .any(|m| (*m - 30.0 * skwir_core::print::POINTS_PER_MM).abs() < 0.01)
            );
            {
                let seen = seen.lock().unwrap();
                assert_eq!(seen.titles, ["Report.pdf"]);
                let options = &seen.options[0];
                assert!(bool::try_from(&options["has_current_page"]).unwrap());
                assert!(!bool::try_from(&options["has_selected_pages"]).unwrap());
            }
            let tmp = tempfile::tempdir().unwrap();
            let pdf = tmp.path().join("made.pdf");
            std::fs::write(&pdf, b"%PDF-1.7 made").unwrap();
            super::send(&bus, None, &chosen, "Report.pdf", &pdf, &cancel).unwrap();
            let seen = seen.lock().unwrap();
            assert_eq!(seen.printed, [b"%PDF-1.7 made".to_vec()]);
            assert_eq!(u32::try_from(&seen.options[1]["token"]).unwrap(), 7);
        }

        #[test]
        fn a_dialog_cancelled_chooses_nothing() {
            let seen = Arc::new(Mutex::new(Seen::default()));
            let (bus, _portal) = serve(Portal { seen, answer: None });
            let chosen = super::ask(&bus, None, &Ask::default(), &CancelToken::new()).unwrap();
            assert!(chosen.is_none());
        }

        /// Something else at the portal's place, as a desktop whose portal
        /// has no print back end.
        struct Elsewhere;

        #[zbus::interface(name = "org.example.Elsewhere")]
        impl Elsewhere {
            #[allow(clippy::unused_self)] // zbus serves a method of an object.
            fn nothing(&self) {}
        }

        #[test]
        fn a_desktop_without_a_print_portal_says_so() {
            let (theirs, ours) = std::os::unix::net::UnixStream::pair().unwrap();
            let server = std::thread::spawn(move || {
                zbus::blocking::connection::Builder::async_io_unix_stream(theirs)
                    .server(zbus::Guid::generate())
                    .unwrap()
                    .p2p()
                    .serve_at(PATH, Elsewhere)
                    .unwrap()
                    .build()
                    .unwrap()
            });
            // A portal that answers nothing is given up on, as the print
            // path's own connection gives it up.
            let bus = zbus::blocking::connection::Builder::async_io_unix_stream(ours)
                .p2p()
                .method_timeout(std::time::Duration::from_secs(5))
                .build()
                .unwrap();
            let _server = server.join().unwrap();
            let error = super::ask(&bus, None, &Ask::default(), &CancelToken::new()).unwrap_err();
            assert!(error.to_string().contains("no print dialog"), "{error:#}");
        }
    }
}

/// macOS's print panel (`NSPrintPanel`), which chooses, then CUPS's `lp`,
/// which prints the file on the printer chosen with its copies and sides.
/// Compiled for macOS from Linux, never run (ADR-075).
#[cfg(target_os = "macos")]
mod macos {
    use super::{Ask, Chosen, Duplex, PageSet, Pages, System, sheet};
    use objc2_app_kit::{
        NSPaperOrientation, NSPrintAllPages, NSPrintCopies, NSPrintFirstPage, NSPrintInfo,
        NSPrintLastPage, NSPrintPanel, NSPrintPanelOptions,
    };
    use objc2_foundation::{NSNumber, NSString};
    use skwir_core::CancelToken;
    use skwir_core::print::{POINTS_PER_MM, Range};
    use std::path::Path;

    /// What the panel chose, as plain values: `AppKit`'s objects stay on
    /// the main thread.
    struct Panel {
        size: (f64, f64),
        margins: [f64; 4],
        landscape: bool,
        range: Option<(i64, i64)>,
        selection: bool,
        copies: i64,
        duplex: i64,
        scale: f64,
        printer: String,
    }

    #[allow(clippy::unnecessary_wraps)] // The other systems' dialogs may fail.
    pub(super) fn ask(ask: &Ask, _cancel: &CancelToken) -> anyhow::Result<Option<Chosen>> {
        let offer_selection = ask.selection;
        // AppKit's panels run on the main thread: this waits for it there,
        // as a modal panel does.
        let panel = dispatch2::run_on_main(move |mtm| {
            let info = NSPrintInfo::sharedPrintInfo();
            let panel = NSPrintPanel::printPanel(mtm);
            let mut options = NSPrintPanelOptions::ShowsCopies
                | NSPrintPanelOptions::ShowsPageRange
                | NSPrintPanelOptions::ShowsPaperSize
                | NSPrintPanelOptions::ShowsOrientation
                | NSPrintPanelOptions::ShowsScaling;
            if offer_selection {
                options |= NSPrintPanelOptions::ShowsPrintSelection;
            }
            panel.setOptions(options);
            // NSModalResponseOK.
            if panel.runModalWithPrintInfo(&info) != 1 {
                return None;
            }
            #[allow(unsafe_code)]
            // SAFETY: the dictionary is NSPrintInfo's own, of attribute
            // keys to objects, as AppKit declares it; the keys are AppKit's
            // constant strings.
            let (all, first, last, copies, duplex) = unsafe {
                let dictionary = info.dictionary();
                let number = |key: &NSString| {
                    dictionary
                        .objectForKey(key)
                        .and_then(|value| value.downcast::<NSNumber>().ok())
                        .map(|number| number.integerValue() as i64)
                };
                let settings = info.printSettings();
                let duplex = settings
                    .objectForKey(&NSString::from_str(
                        "com_apple_print_PrintSettings_PMDuplexing",
                    ))
                    .and_then(|value| value.downcast::<NSNumber>().ok())
                    .map_or(1, |number| number.integerValue() as i64);
                (
                    number(NSPrintAllPages).unwrap_or(1) != 0,
                    number(NSPrintFirstPage).unwrap_or(1),
                    number(NSPrintLastPage).unwrap_or(i64::MAX),
                    number(NSPrintCopies).unwrap_or(1),
                    duplex,
                )
            };
            let size = info.paperSize();
            Some(Panel {
                size: (size.width, size.height),
                margins: [
                    info.topMargin(),
                    info.rightMargin(),
                    info.bottomMargin(),
                    info.leftMargin(),
                ],
                landscape: info.orientation() == NSPaperOrientation::Landscape,
                range: (!all).then_some((first, last)),
                selection: info.isSelectionOnly(),
                copies,
                duplex,
                scale: info.scalingFactor(),
                printer: info.printer().name().to_string(),
            })
        });
        let Some(panel) = panel else {
            return Ok(None);
        };
        // AppKit measures in points; `sheet` takes millimetres, portrait.
        let mm = |points: f64| points as f32 / POINTS_PER_MM;
        let (width, height) = if panel.landscape {
            (panel.size.1, panel.size.0)
        } else {
            panel.size
        };
        let pages = if panel.selection {
            Pages::Selection
        } else if let Some((first, last)) = panel.range {
            let page = |n: i64| u32::try_from(n.max(1)).unwrap_or(u32::MAX);
            Pages::Ranges(vec![Range {
                first: page(first),
                last: (last < i64::from(u32::MAX)).then(|| page(last)),
            }])
        } else {
            Pages::All
        };
        Ok(Some(Chosen {
            pages,
            page_set: PageSet::All,
            reverse: false,
            scale: (panel.scale as f32).clamp(0.1, 10.0),
            paper: sheet(
                mm(width),
                mm(height),
                panel.landscape,
                panel.margins.map(mm),
            ),
            // PMDuplexing: 1 none, 2 the long edge, 3 the short edge.
            duplex: match panel.duplex {
                2 => Duplex::LongEdge,
                3 => Duplex::ShortEdge,
                _ => Duplex::Simplex,
            },
            printer: Some(panel.printer.clone()),
            to_file: None,
            system: System::Cups {
                printer: panel.printer,
                copies: u32::try_from(panel.copies.max(1)).unwrap_or(1),
            },
        }))
    }

    pub(super) fn send(
        chosen: &Chosen,
        title: &str,
        pdf: &Path,
        cancel: &CancelToken,
    ) -> anyhow::Result<()> {
        // On macOS what the panel keeps is the printer and its copies.
        let System::Cups { printer, copies } = &chosen.system;
        let sides = match chosen.duplex {
            Duplex::Simplex => "one-sided",
            Duplex::LongEdge => "two-sided-long-edge",
            Duplex::ShortEdge => "two-sided-short-edge",
        };
        let mut lp = std::process::Command::new("/usr/bin/lp");
        lp.args(["-d", printer, "-n", &copies.to_string(), "-t", title])
            .args(["-o", &format!("sides={sides}")])
            .arg("--")
            .arg(pdf);
        crate::process::run(&mut lp, cancel, std::time::Duration::from_secs(120))
            .map(|_| ())
            .map_err(|error| anyhow::anyhow!("the printer refused it: {error:#}"))
    }
}

/// Windows' print dialog (`PrintDlgExW`), which chooses, then the PDF
/// helper drawing the pages on the printer with the settings the dialog
/// left (`document::print_on`). Compiled for Windows from Linux, tried
/// under Wine (ADR-074), which has no printer.
#[cfg(windows)]
mod windows {
    use super::{Ask, Chosen, Duplex, PageSet, Pages, System, sheet};
    use ::windows::Win32::Foundation::{GlobalFree, HGLOBAL};
    use ::windows::Win32::Graphics::Gdi::{
        DEVMODEW, DM_PAPERLENGTH, DM_PAPERWIDTH, DMDUP_HORIZONTAL, DMDUP_VERTICAL,
        DMORIENT_LANDSCAPE,
    };
    use ::windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
    use ::windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
    use ::windows::Win32::UI::Controls::Dialogs::{
        DEVNAMES, PD_CURRENTPAGE, PD_NOCURRENTPAGE, PD_NOSELECTION, PD_PAGENUMS, PD_RESULT_PRINT,
        PD_SELECTION, PD_USEDEVMODECOPIESANDCOLLATE, PRINTDLGEXW, PRINTPAGERANGE, PrintDlgExW,
        START_PAGE_GENERAL,
    };
    use ::windows::Win32::UI::WindowsAndMessaging::GetDesktopWindow;
    use skwir_core::CancelToken;
    use skwir_core::print::Range;
    use std::path::Path;

    /// Ranges a dialog may give at most.
    const RANGES: usize = 64;

    /// What the dialog chose, as plain values.
    struct Dialog {
        current: bool,
        selection: bool,
        ranges: Vec<(u32, u32)>,
        devmode: Vec<u8>,
        printer: String,
    }

    pub(super) fn ask(ask: &Ask, _cancel: &CancelToken) -> anyhow::Result<Option<Chosen>> {
        let (current, selection) = (ask.current_page, ask.selection);
        // The dialog is a COM property sheet: shown on a thread of its own,
        // in a single-threaded apartment, the worker's left as it was.
        let Some(dialog) = std::thread::Builder::new()
            .name("skwir-print-dialog".into())
            .spawn(move || dialog(current, selection))?
            .join()
            .map_err(|_| anyhow::anyhow!("the print dialog stopped"))??
        else {
            return Ok(None);
        };
        let mode = read_devmode(&dialog.devmode)?;
        // SAFETY of the union read: `dmFields` says which are set; every
        // member of this one is a plain number.
        #[allow(unsafe_code)]
        let printer_part = unsafe { mode.Anonymous1.Anonymous1 };
        let tenths = |n: i16| f32::from(n) / 10.0;
        let fallback = super::paper();
        let landscape = u32::try_from(printer_part.dmOrientation) == Ok(DMORIENT_LANDSCAPE);
        let paper = if mode.dmFields.contains(DM_PAPERWIDTH)
            && mode.dmFields.contains(DM_PAPERLENGTH)
            && printer_part.dmPaperWidth > 0
            && printer_part.dmPaperLength > 0
        {
            sheet(
                tenths(printer_part.dmPaperWidth),
                tenths(printer_part.dmPaperLength),
                landscape,
                [0.0; 4],
            )
        } else {
            let named = match printer_part.dmPaperSize {
                1 => Some((215.9, 279.4)),
                5 => Some((215.9, 355.6)),
                8 => Some((297.0, 420.0)),
                9 => Some((210.0, 297.0)),
                11 => Some((148.0, 210.0)),
                _ => None,
            };
            match named {
                Some((width, height)) => sheet(width, height, landscape, [0.0; 4]),
                None if landscape => fallback.landscape(),
                None => fallback,
            }
        };
        let pages = if dialog.selection {
            Pages::Selection
        } else if dialog.current {
            Pages::Current
        } else if dialog.ranges.is_empty() {
            Pages::All
        } else {
            Pages::Ranges(
                dialog
                    .ranges
                    .iter()
                    .map(|&(first, last)| Range {
                        first: first.max(1),
                        last: Some(last.max(first.max(1))),
                    })
                    .collect(),
            )
        };
        Ok(Some(Chosen {
            pages,
            page_set: PageSet::All,
            reverse: false,
            scale: 1.0,
            paper,
            // Windows' "vertical" turns over the long edge of a portrait
            // sheet, "horizontal" over its short edge.
            duplex: if mode.dmDuplex == DMDUP_VERTICAL {
                Duplex::LongEdge
            } else if mode.dmDuplex == DMDUP_HORIZONTAL {
                Duplex::ShortEdge
            } else {
                Duplex::Simplex
            },
            printer: Some(dialog.printer.clone()),
            to_file: None,
            system: System::Printer {
                name: dialog.printer,
                devmode: dialog.devmode,
            },
        }))
    }

    /// The settings as a `DEVMODEW`, its size checked.
    fn read_devmode(bytes: &[u8]) -> anyhow::Result<DEVMODEW> {
        anyhow::ensure!(
            bytes.len() >= std::mem::size_of::<DEVMODEW>(),
            "the printer's settings are not whole"
        );
        #[allow(unsafe_code)]
        // SAFETY: `bytes` holds at least a DEVMODEW, a plain C structure;
        // the read does not ask it to be aligned.
        Ok(unsafe { std::ptr::read_unaligned(bytes.as_ptr().cast::<DEVMODEW>()) })
    }

    /// Show the dialog: what it chose, `None` when cancelled.
    #[allow(unsafe_code)]
    fn dialog(current: bool, selection: bool) -> anyhow::Result<Option<Dialog>> {
        let mut ranges = [PRINTPAGERANGE::default(); RANGES];
        let mut flags = PD_USEDEVMODECOPIESANDCOLLATE;
        if !current {
            flags |= PD_NOCURRENTPAGE;
        }
        if !selection {
            flags |= PD_NOSELECTION;
        }
        // SAFETY: COM is started on this thread and ended here; the dialog
        // is handed a structure of its own size, a valid owner window and
        // the range array it may fill (its length given); the memory it
        // gives back is read while locked, copied, and freed here.
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
            let mut dialog = PRINTDLGEXW {
                lStructSize: u32::try_from(std::mem::size_of::<PRINTDLGEXW>()).unwrap_or(u32::MAX),
                hwndOwner: GetDesktopWindow(),
                Flags: flags,
                nMaxPageRanges: RANGES as u32,
                lpPageRanges: ranges.as_mut_ptr(),
                nMinPage: 1,
                nMaxPage: 0xFFFF,
                nCopies: 1,
                nStartPage: START_PAGE_GENERAL,
                ..Default::default()
            };
            let shown = PrintDlgExW(std::ptr::from_mut(&mut dialog));
            let chosen = shown.is_ok() && dialog.dwResultAction == PD_RESULT_PRINT;
            let result = if chosen {
                let devmode = locked_bytes(dialog.hDevMode);
                let printer = printer_name(dialog.hDevNames);
                Ok(Some(Dialog {
                    current: dialog.Flags.contains(PD_CURRENTPAGE),
                    selection: dialog.Flags.contains(PD_SELECTION),
                    ranges: if dialog.Flags.contains(PD_PAGENUMS) {
                        ranges[..(dialog.nPageRanges as usize).min(RANGES)]
                            .iter()
                            .map(|range| (range.nFromPage, range.nToPage))
                            .collect()
                    } else {
                        Vec::new()
                    },
                    devmode,
                    printer: printer.unwrap_or_default(),
                }))
            } else {
                shown.map(|()| None).map_err(anyhow::Error::from)
            };
            for handle in [dialog.hDevMode, dialog.hDevNames] {
                if !handle.is_invalid() {
                    let _ = GlobalFree(Some(handle));
                }
            }
            CoUninitialize();
            result
        }
    }

    /// The bytes a global memory handle holds.
    #[allow(unsafe_code)]
    unsafe fn locked_bytes(handle: HGLOBAL) -> Vec<u8> {
        if handle.is_invalid() {
            return Vec::new();
        }
        // SAFETY: the caller hands a handle the dialog allocated; it is
        // locked for the copy and unlocked after.
        unsafe {
            let size = GlobalSize(handle);
            let at = GlobalLock(handle).cast::<u8>();
            if at.is_null() {
                return Vec::new();
            }
            let bytes = std::slice::from_raw_parts(at, size).to_vec();
            let _ = GlobalUnlock(handle);
            bytes
        }
    }

    /// The printer's name in the dialog's `DEVNAMES`.
    #[allow(unsafe_code)]
    unsafe fn printer_name(handle: HGLOBAL) -> Option<String> {
        // SAFETY: as `locked_bytes`.
        let bytes = unsafe { locked_bytes(handle) };
        if bytes.len() < std::mem::size_of::<DEVNAMES>() {
            return None;
        }
        // SAFETY: `bytes` holds at least a DEVNAMES; read unaligned.
        let names = unsafe { std::ptr::read_unaligned(bytes.as_ptr().cast::<DEVNAMES>()) };
        // Offsets count characters (UTF-16) from the structure's start.
        let words: Vec<u16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect();
        let from = usize::from(names.wDeviceOffset);
        let name = words.get(from..)?;
        let end = name.iter().position(|&c| c == 0).unwrap_or(name.len());
        Some(String::from_utf16_lossy(&name[..end]))
    }

    pub(super) fn send(
        chosen: &Chosen,
        title: &str,
        pdf: &Path,
        cancel: &CancelToken,
    ) -> anyhow::Result<()> {
        // On Windows what the dialog keeps is the printer and its settings.
        let System::Printer { name, devmode } = &chosen.system;
        crate::document::print_on(pdf, name, devmode, title, None, cancel)?
            .ok_or_else(|| anyhow::anyhow!("printing needs PDFium, which is not there"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_paper_is_the_locale_s_unless_libpaper_says() {
        assert_eq!(paper_of_locale("en_US.UTF-8"), Paper::letter());
        assert_eq!(paper_of_locale("fr_CA.UTF-8@euro"), Paper::letter());
        assert_eq!(paper_of_locale("fr_FR.UTF-8"), Paper::a4());
        assert_eq!(paper_of_locale("C"), Paper::a4());
        assert_eq!(paper_named("# a comment\nletter\n"), Some(Paper::letter()));
        assert_eq!(paper_named("a4"), Some(Paper::a4()));
        assert_eq!(paper_named("folio"), None);
    }
}
