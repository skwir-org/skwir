//! Smaller copies of pictures and PDFs (ADR-112): the qualities offered,
//! what each does to a picture and to a PDF's images, and what the shrink
//! helper (`skwir-print-worker --shrink <job.json>`) is asked and answers.
//! Pure: the engine is `skwir-shrink`, the command the `shrink` plugin's.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// How small a copy is made, from the copy that loses nothing to the
/// smallest. Each says what it does to a picture ([`Quality::picture`])
/// and to a PDF's images ([`Quality::pdf`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Quality {
    /// Nothing re-encoded at a loss: what is stored twice kept once, the
    /// rest packed tighter.
    Lossless,
    /// For paper: pictures large, a PDF's images at most 300 dpi.
    Print,
    /// For a screen: pictures to 1920 pixels, a PDF's images to 150 dpi.
    #[default]
    Screen,
    /// For a message: pictures to 1280 pixels, a PDF's images to 96 dpi.
    Small,
}

/// What a quality does to a picture: its longer side at most (none, as it
/// is: a picture is never enlarged), and the JPEG quality it is written at
/// (none, nothing re-encoded at a loss).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PictureRule {
    pub longest: Option<u32>,
    pub jpeg: Option<u8>,
}

/// What a quality does to a PDF's images: the most dots per inch kept, an
/// image above `from` brought down to `dpi` (none, as it is); the JPEG
/// quality an image is written at (none, nothing re-encoded at a loss);
/// and whether an image stored without loss (a screenshot, a page drawn)
/// may be written as JPEG where that is smaller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PdfRule {
    pub dpi: Option<u32>,
    pub from: Option<u32>,
    pub jpeg: Option<u8>,
    pub lossy_flat: bool,
}

impl Quality {
    pub const ALL: [Quality; 4] = [
        Quality::Lossless,
        Quality::Print,
        Quality::Screen,
        Quality::Small,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Quality::Lossless => "lossless",
            Quality::Print => "print",
            Quality::Screen => "screen",
            Quality::Small => "small",
        }
    }

    pub fn of_key(key: &str) -> Option<Quality> {
        Quality::ALL
            .into_iter()
            .find(|quality| quality.key() == key)
    }

    pub fn title(self) -> &'static str {
        match self {
            Quality::Lossless => "Lossless",
            Quality::Print => "Print",
            Quality::Screen => "Screen",
            Quality::Small => "Small",
        }
    }

    pub fn picture(self) -> PictureRule {
        match self {
            Quality::Lossless => PictureRule {
                longest: None,
                jpeg: None,
            },
            Quality::Print => PictureRule {
                longest: Some(3840),
                jpeg: Some(85),
            },
            Quality::Screen => PictureRule {
                longest: Some(1920),
                jpeg: Some(80),
            },
            Quality::Small => PictureRule {
                longest: Some(1280),
                jpeg: Some(70),
            },
        }
    }

    pub fn pdf(self) -> PdfRule {
        match self {
            Quality::Lossless => PdfRule {
                dpi: None,
                from: None,
                jpeg: None,
                lossy_flat: false,
            },
            Quality::Print => PdfRule {
                dpi: Some(300),
                from: Some(450),
                jpeg: Some(85),
                lossy_flat: false,
            },
            Quality::Screen => PdfRule {
                dpi: Some(150),
                from: Some(225),
                jpeg: Some(75),
                lossy_flat: true,
            },
            Quality::Small => PdfRule {
                dpi: Some(96),
                from: Some(144),
                jpeg: Some(60),
                lossy_flat: true,
            },
        }
    }

    /// What it does to a picture, said under its name in a dialog.
    pub fn picture_said(self) -> String {
        let rule = self.picture();
        match rule.longest {
            Some(longest) => format!("At most {longest} pixels across"),
            None => "Same pixels, nothing lost".to_owned(),
        }
    }

    /// What it does to a PDF, said under its name in a dialog.
    pub fn pdf_said(self) -> String {
        let rule = self.pdf();
        match rule.dpi {
            Some(dpi) => format!("Images at most {dpi} dpi"),
            None => "Nothing re-encoded".to_owned(),
        }
    }
}

/// The steps tried, below the quality chosen, to bring a copy under the
/// size asked (`Job::at_most`): for a picture, its longer side and JPEG
/// quality; for a PDF, its images' dpi and JPEG quality.
pub const PICTURE_STEPS: [PictureRule; 3] = [
    PictureRule {
        longest: Some(1024),
        jpeg: Some(60),
    },
    PictureRule {
        longest: Some(800),
        jpeg: Some(50),
    },
    PictureRule {
        longest: Some(640),
        jpeg: Some(40),
    },
];

pub const PDF_STEPS: [PdfRule; 2] = [
    PdfRule {
        dpi: Some(72),
        from: Some(96),
        jpeg: Some(50),
        lossy_flat: true,
    },
    PdfRule {
        dpi: Some(50),
        from: Some(66),
        jpeg: Some(40),
        lossy_flat: true,
    },
];

/// What the shrink helper is asked: a smaller copy of `input` at `output`,
/// or none when it would not be smaller.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Job {
    pub kind: Kind,
    pub input: PathBuf,
    pub output: PathBuf,
    pub quality: Quality,
    /// Pictures, and a PDF's images, made gray.
    #[serde(default, skip_serializing_if = "is_false")]
    pub gray: bool,
    /// A picture's location and camera details left out: its date stays.
    #[serde(default, skip_serializing_if = "is_false")]
    pub strip: bool,
    /// At most this many bytes: smaller steps are tried until it fits
    /// ([`PICTURE_STEPS`], [`PDF_STEPS`]), the smallest kept when none does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at_most: Option<u64>,
    /// A PDF's images (by object number) kept as they were: those a page
    /// that looked different showed, after a first copy was checked.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keep: Vec<u32>,
}

/// What is made smaller.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Picture,
    Pdf,
}

/// The helper's answer, a line on its standard output.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Made {
    /// The input's size, in bytes.
    pub before: u64,
    /// The copy's size; the input's when none was written.
    pub after: u64,
    /// Whether a copy was written: none when it would not be smaller.
    pub written: bool,
    /// Whether it fits `at_most`, when asked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub met: Option<bool>,
    /// A PDF's images written anew, and the pages (from 0) that show each.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed: Vec<Changed>,
    /// What was left as it was, and why: said to the user.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

/// A PDF's image written anew: its object number, the pages showing it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Changed {
    pub object: u32,
    pub pages: Vec<u32>,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(value: &bool) -> bool {
    !*value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qualities_go_from_lossless_to_smallest() {
        let dpis: Vec<_> = Quality::ALL.iter().filter_map(|q| q.pdf().dpi).collect();
        assert!(dpis.windows(2).all(|pair| pair[0] > pair[1]));
        let sides: Vec<_> = Quality::ALL
            .iter()
            .filter_map(|q| q.picture().longest)
            .collect();
        assert!(sides.windows(2).all(|pair| pair[0] > pair[1]));
        let smallest = Quality::Small.picture().longest.unwrap_or(0);
        assert!(
            PICTURE_STEPS
                .iter()
                .all(|step| step.longest < Some(smallest))
        );
        let lowest = Quality::Small.pdf().dpi.unwrap_or(0);
        assert!(PDF_STEPS.iter().all(|step| step.dpi < Some(lowest)));
        for quality in Quality::ALL {
            assert_eq!(Quality::of_key(quality.key()), Some(quality));
            let rule = quality.pdf();
            assert_eq!(rule.dpi.is_some(), rule.from.is_some());
            assert!(rule.from >= rule.dpi);
        }
    }

    #[test]
    fn a_job_leaves_out_what_is_at_its_default() {
        let job = Job {
            kind: Kind::Pdf,
            input: "/example/in.pdf".into(),
            output: "/example/out.pdf".into(),
            quality: Quality::Screen,
            gray: false,
            strip: false,
            at_most: None,
            keep: Vec::new(),
        };
        let text = serde_json::to_string(&job).unwrap();
        assert!(!text.contains("gray") && !text.contains("keep"));
        let back: Job = serde_json::from_str(&text).unwrap();
        assert_eq!(back.quality, Quality::Screen);
    }
}
