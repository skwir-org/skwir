//! Reduce Size…'s dialog (ADR-112): how small (four cards, each saying
//! what it does to what is chosen: pictures, PDFs, or both), gray, a
//! picture's location and camera details left out, and at most a size,
//! asked as a form the window draws (`skwir_core::form`). It opens on
//! what was chosen last, kept in memory until Skwir quits or the plugin is
//! turned off, never a setting.
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use skwir_core::App;
use skwir_core::cancel::CancelToken;
use skwir_core::form::{Answer, Choice, Field, Form, Kind, Values};
use skwir_core::shrink::Quality;

/// What the dialog may offer, known before it asks.
#[derive(Clone, Debug, Default)]
pub(crate) struct Offers {
    /// What is made smaller, as its title says it: “report.pdf”, 3 files.
    pub what: String,
    /// Pictures are among them: the location and camera details offered.
    pub pictures: bool,
    /// PDFs are among them.
    pub pdfs: bool,
    /// They are all JPEGs, which Lossless leaves as they are but for what
    /// they say of where and with what they were taken.
    pub only_jpegs: bool,
}

/// What the dialog chose.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Chosen {
    pub quality: Quality,
    pub gray: bool,
    pub strip: bool,
    /// Bytes each copy fits in, when asked.
    pub at_most: Option<u64>,
}

/// What the next dialog opens on.
#[derive(Clone, Debug, Default)]
struct Remembered {
    quality: Quality,
    gray: bool,
    strip: bool,
    at_most: String,
}

/// The last dialog's choices: the plugin's own, for the app it is added
/// to (`ShrinkPlugin`), shared by what it registers there.
#[derive(Clone, Default)]
pub(crate) struct Last(Arc<Mutex<Option<Remembered>>>);

impl Last {
    fn held(&self) -> MutexGuard<'_, Option<Remembered>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Forget the last choices: the plugin turned off leaves no trace.
    pub(crate) fn forget(&self) {
        *self.held() = None;
    }
}

/// The form's fields, by key.
const QUALITY: &str = "quality";
const GRAY: &str = "gray";
const STRIP: &str = "strip";
const AT_MOST: &str = "at_most";

/// The qualities that lose something: what gray and a size asked apply to.
const LOSSY: [&str; 3] = ["print", "screen", "small"];

/// Ask the dialog until it is answered with choices that hold: `None`
/// when it was cancelled, or no window can show it.
pub(crate) fn ask(app: &App, offers: &Offers, last: &Last, cancel: &CancelToken) -> Option<Chosen> {
    let opened = last.held().clone().unwrap_or_default();
    let mut form = form(offers, &opened);
    loop {
        let Some(Answer::Accepted(values)) = app.fill(form.clone(), cancel) else {
            return None;
        };
        match chosen(&values, offers) {
            Ok(chosen) => {
                *last.held() = Some(Remembered {
                    quality: chosen.quality,
                    gray: chosen.gray,
                    strip: chosen.strip,
                    at_most: values.get(AT_MOST).trim().to_owned(),
                });
                return Some(chosen);
            }
            Err(refused) => form = form.again(&values, refused),
        }
    }
}

/// The dialog for `offers`, on the `last` choices.
fn form(offers: &Offers, last: &Remembered) -> Form {
    let several = !offers.what.starts_with('“');
    let mut form = Form::new(
        format!("Reduce Size of {}", offers.what),
        if several { "Make Copies" } else { "Make Copy" },
    )
    .intro(if several {
        "A smaller copy is made beside each file; the files stay as they are."
    } else {
        "A smaller copy is made beside it; the file stays as it is."
    });
    let cards = Quality::ALL
        .into_iter()
        .map(|quality| Choice::new(quality.key(), quality.title()).detail(said(quality, offers)))
        .collect();
    form = form
        .field(
            Field::new(QUALITY, "Quality", Kind::Row { choices: cards }).value(last.quality.key()),
        )
        .field(
            Field::new(GRAY, "In gray", Kind::Toggle)
                .help(if offers.pdfs {
                    "Pictures, and a PDF's images, in shades of gray"
                } else {
                    "In shades of gray"
                })
                .value(last.gray.to_string())
                .when(QUALITY, &LOSSY),
        );
    if offers.pictures {
        form = form.field(
            Field::new(STRIP, "Remove location and camera details", Kind::Toggle)
                .help("The date it was taken stays")
                .value(last.strip.to_string()),
        );
    }
    form.field(
        Field::text(AT_MOST, "At most")
            .placeholder("No limit, or a size: 2 MB, 500 KB")
            .value(last.at_most.clone())
            .when(QUALITY, &LOSSY),
    )
}

/// What `quality` does to what is offered, said on its card.
fn said(quality: Quality, offers: &Offers) -> String {
    match (offers.pictures, offers.pdfs) {
        (true, true) => match quality {
            Quality::Lossless => "Nothing re-encoded".to_owned(),
            _ => format!(
                "{}; pictures {} pixels",
                quality.pdf_said(),
                quality.picture().longest.unwrap_or(0)
            ),
        },
        (true, false) => quality.picture_said(),
        _ => quality.pdf_said(),
    }
}

/// What the dialog's `values` chose, or why they are no answer.
fn chosen(values: &Values, offers: &Offers) -> Result<Chosen, String> {
    let quality = Quality::of_key(values.get(QUALITY)).unwrap_or_default();
    let lossy = quality != Quality::Lossless;
    let strip = offers.pictures && values.on(STRIP);
    if !lossy && offers.only_jpegs && !strip {
        return Err(
            "Lossless leaves a JPEG as it is: choose Print, Screen or Small, or remove its location and camera details"
                .to_owned(),
        );
    }
    let at_most = match values.get(AT_MOST).trim() {
        "" => None,
        typed if lossy => Some(size(typed).map_err(|refused| format!("At most: {refused}"))?),
        _ => None,
    };
    Ok(Chosen {
        quality,
        gray: lossy && values.on(GRAY),
        strip,
        at_most,
    })
}

/// A size typed: `2 MB`, `1.5MB`, `500 KB`, `800k`, bytes. Units are
/// those a file manager shows, a thousand to the next.
pub(crate) fn size(typed: &str) -> Result<u64, String> {
    let typed = typed.trim();
    let split = typed
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == ','))
        .unwrap_or(typed.len());
    let (number, unit) = typed.split_at(split);
    let number: f64 = number
        .replace(',', ".")
        .parse()
        .map_err(|_| format!("“{typed}” is no size"))?;
    let scale = match unit.trim().to_ascii_lowercase().as_str() {
        "" | "b" | "bytes" => 1.0,
        "k" | "kb" | "ko" => 1e3,
        "m" | "mb" | "mo" => 1e6,
        "g" | "gb" | "go" => 1e9,
        other => return Err(format!("“{other}” is no unit: write KB or MB")),
    };
    let bytes = number * scale;
    if !(bytes >= 1.0 && bytes.is_finite()) {
        return Err(format!("“{typed}” is no size"));
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Ok(bytes as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offers(pictures: bool, pdfs: bool) -> Offers {
        Offers {
            what: "“example.pdf”".to_owned(),
            pictures,
            pdfs,
            only_jpegs: pictures && !pdfs,
        }
    }

    #[test]
    fn sizes_are_read_as_typed() {
        assert_eq!(size("2 MB"), Ok(2_000_000));
        assert_eq!(size("1,5mb"), Ok(1_500_000));
        assert_eq!(size("500 KB"), Ok(500_000));
        assert_eq!(size("800k"), Ok(800_000));
        assert_eq!(size("1200"), Ok(1200));
        assert!(size("lots").is_err());
        assert!(size("2 parsecs").is_err());
        assert!(size("0").is_err());
    }

    #[test]
    fn the_cards_say_what_they_do_to_what_is_chosen() {
        assert_eq!(
            said(Quality::Screen, &offers(false, true)),
            "Images at most 150 dpi"
        );
        assert_eq!(
            said(Quality::Screen, &offers(true, false)),
            "At most 1920 pixels across"
        );
        assert!(said(Quality::Small, &offers(true, true)).contains("1280"));
    }

    #[test]
    fn location_is_offered_only_for_pictures() {
        let has = |form: &Form, key: &str| form.fields.iter().any(|field| field.key == key);
        let opened = |offers: &Offers| form(offers, &Remembered::default());
        assert!(!has(&opened(&offers(false, true)), STRIP));
        assert!(has(&opened(&offers(true, false)), STRIP));
    }

    #[test]
    fn lossless_jpegs_ask_for_something_to_leave_out() {
        let mut values = Values::default();
        values.set(QUALITY, "lossless");
        assert!(chosen(&values, &offers(true, false)).is_err());
        values.set(STRIP, "true");
        let chosen = chosen(&values, &offers(true, false)).unwrap();
        assert!(chosen.strip && !chosen.gray && chosen.at_most.is_none());
    }
}
