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
use skwir_core::words;
use skwir_core::words::{Name, Words};

/// What the dialog may offer, known before it asks.
#[derive(Clone, Debug, Default)]
pub(crate) struct Offers {
    /// How many files are made smaller, and the first's name: what its
    /// title says (“report.pdf”, 3 files).
    pub count: usize,
    pub first: String,
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

/// The dialog for `offers`, on the `last` choices. Its words take the
/// file it is about, or how many.
fn form(offers: &Offers, last: &Remembered) -> Form {
    let (count, first) = (offers.count, || Name(offers.first.clone()));
    let mut form = Form::new(
        words!(
            shrink_dialog_title = "{ $count ->
                [1] Reduce Size of { $name }
               *[other] Reduce Size of { $count } files
            }",
            count = count,
            name = first(),
            note = "The title of the dialog asking how small copies of pictures and PDFs are made."
        ),
        words!(
            shrink_dialog_accept = "{ $count ->
                [1] Make Copy
               *[other] Make Copies
            }",
            count = count,
            note = "The dialog's button: makes the smaller copy, or copies."
        ),
    )
    .intro(words!(
        shrink_dialog_intro = "{ $count ->
            [1] A smaller copy is made beside it; the file stays as it is.
           *[other] A smaller copy is made beside each file; the files stay as they are.
        }",
        count = count
    ));
    let cards = Quality::ALL
        .into_iter()
        .map(|quality| Choice::new(quality.key(), title_of(quality)).detail(said(quality, offers)))
        .collect();
    form = form
        .field(
            Field::new(
                QUALITY,
                words!(
                    shrink_quality = "Quality",
                    note = "A field of the dialog: four cards, from Lossless to Small."
                ),
                Kind::Row { choices: cards },
            )
            .value(last.quality.key()),
        )
        .field(
            Field::new(
                GRAY,
                words!(
                    shrink_gray = "In gray",
                    note = "A switch of the dialog: copies made in shades of gray."
                ),
                Kind::Toggle,
            )
            .help(if offers.pdfs {
                words!(shrink_gray_help_pdfs = "Pictures, and a PDF's images, in shades of gray")
            } else {
                words!(shrink_gray_help = "In shades of gray")
            })
            .value(last.gray.to_string())
            .when(QUALITY, &LOSSY),
        );
    if offers.pictures {
        form = form.field(
            Field::new(
                STRIP,
                words!(
                    shrink_strip = "Remove location and camera details",
                    note = "A switch of the dialog: what a picture says of where and with what it was taken is left out of its copy."
                ),
                Kind::Toggle,
            )
            .help(words!(
                shrink_strip_help = "The date it was taken stays",
                note = "Under the switch removing a picture's location and camera details."
            ))
            .value(last.strip.to_string()),
        );
    }
    form.field(
        Field::text(
            AT_MOST,
            words!(
                shrink_at_most = "At most",
                note = "A field of the dialog: the size each copy fits in."
            ),
        )
        .placeholder(words!(
            shrink_at_most_placeholder = "No limit, or a size: 2 MB, 500 KB",
            note = "In the empty At most field. The sizes are examples of what may be typed, with their units as written."
        ))
        .value(last.at_most.clone())
        .when(QUALITY, &LOSSY),
    )
}

/// A quality's card's title.
fn title_of(quality: Quality) -> Words {
    match quality {
        Quality::Lossless => words!(
            shrink_lossless = "Lossless",
            note = "A card of the dialog: nothing re-encoded, what is stored twice kept once."
        ),
        Quality::Print => words!(
            shrink_print = "Print",
            note = "A card of the dialog: copies good enough to print."
        ),
        Quality::Screen => words!(
            shrink_screen = "Screen",
            note = "A card of the dialog: copies good enough for a screen."
        ),
        Quality::Small => words!(
            shrink_small = "Small",
            note = "A card of the dialog: the smallest copies."
        ),
    }
}

/// What `quality` does to what is offered, said on its card: to the
/// pictures, the most pixels across; to a PDF's images, the most dots per
/// inch.
fn said(quality: Quality, offers: &Offers) -> Words {
    let pixels = quality.picture().longest;
    let dpi = quality.pdf().dpi;
    let nothing_re_encoded = || {
        words!(
            shrink_nothing_re_encoded = "Nothing re-encoded",
            note = "On the Lossless card: no picture or image is written again at a loss."
        )
    };
    let pdf = || match dpi {
        Some(dpi) => words!(
            shrink_pdf_dpi = "Images at most { $dpi } dpi",
            dpi = dpi,
            note = "On a card: a PDF's images brought down to this many dots per inch."
        ),
        None => nothing_re_encoded(),
    };
    match (offers.pictures, offers.pdfs) {
        (true, true) => match (quality, dpi) {
            (Quality::Lossless, _) | (_, None) => nothing_re_encoded(),
            (_, Some(dpi)) => words!(
                shrink_both = "{ $pixels ->
                    [one] Images at most { $dpi } dpi; pictures { $pixels } pixel
                   *[other] Images at most { $dpi } dpi; pictures { $pixels } pixels
                }",
                dpi = dpi,
                pixels = pixels.unwrap_or(0),
                note = "On a card, where pictures and PDFs are chosen: a PDF's images brought down to this many dots per inch, pictures to this many pixels across."
            ),
        },
        (true, false) => match pixels {
            Some(pixels) => words!(
                shrink_picture_pixels = "{ $pixels ->
                    [one] At most { $pixels } pixel across
                   *[other] At most { $pixels } pixels across
                }",
                pixels = pixels,
                note =
                    "On a card: pictures brought down to this many pixels on their longest side."
            ),
            None => words!(
                shrink_same_pixels = "Same pixels, nothing lost",
                note = "On the Lossless card, where pictures are chosen."
            ),
        },
        _ => pdf(),
    }
}

/// What the dialog's `values` chose, or why they are no answer.
fn chosen(values: &Values, offers: &Offers) -> Result<Chosen, Words> {
    let quality = Quality::of_key(values.get(QUALITY)).unwrap_or_default();
    let lossy = quality != Quality::Lossless;
    let strip = offers.pictures && values.on(STRIP);
    if !lossy && offers.only_jpegs && !strip {
        return Err(words!(
            shrink_lossless_jpeg = "Lossless leaves a JPEG as it is: choose Print, Screen or Small, or remove its location and camera details",
            note = "Said in the dialog; Print, Screen and Small are its cards' titles, and the last words its switch's."
        ));
    }
    let at_most = match values.get(AT_MOST).trim() {
        "" => None,
        typed if lossy => Some(size(typed).map_err(|refused| {
            words!(
                shrink_at_most_refused = "At most: { $why }",
                why = refused,
                note = "Said in the dialog: why what was typed in the At most field is no size."
            )
        })?),
        _ => None,
    };
    Ok(Chosen {
        quality,
        gray: lossy && values.on(GRAY),
        strip,
        at_most,
    })
}

/// A size typed: `2 MB`, `1.5MB`, `500 KB`, `800k`, bytes, a thousand
/// to the next; `2 MiB`, `500 KiB`, 1024 to the next (ADR-139, 9).
pub(crate) fn size(typed: &str) -> Result<u64, Words> {
    let typed = typed.trim();
    let no_size = || {
        words!(
            shrink_no_size = "“{ $typed }” is no size",
            typed = typed.to_owned(),
            note = "What was typed for a size, as typed."
        )
    };
    let split = typed
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == ','))
        .unwrap_or(typed.len());
    let (number, unit) = typed.split_at(split);
    let number: f64 = number.replace(',', ".").parse().map_err(|_| no_size())?;
    let scale = match unit.trim().to_ascii_lowercase().as_str() {
        "" | "b" | "bytes" => 1.0,
        "k" | "kb" | "ko" => 1e3,
        "m" | "mb" | "mo" => 1e6,
        "g" | "gb" | "go" => 1e9,
        "kib" => 1024.0,
        "mib" => 1_048_576.0,
        "gib" => 1_073_741_824.0,
        other => {
            return Err(words!(
                shrink_no_unit = "“{ $unit }” is no unit: write KB or MB",
                unit = other.to_owned(),
                note = "The unit typed after a size, as typed; KB and MB are units as written."
            ));
        }
    };
    let bytes = number * scale;
    if !(bytes >= 1.0 && bytes.is_finite()) {
        return Err(no_size());
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Ok(bytes as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offers(pictures: bool, pdfs: bool) -> Offers {
        Offers {
            count: 1,
            first: "example.pdf".to_owned(),
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
            said(Quality::Screen, &offers(false, true)).english(),
            "Images at most 150 dpi"
        );
        assert_eq!(
            said(Quality::Screen, &offers(true, false)).english(),
            "At most 1,920 pixels across"
        );
        assert!(
            said(Quality::Small, &offers(true, true))
                .english()
                .contains("1,280")
        );
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
