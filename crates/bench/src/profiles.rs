//! The synthetic tree shapes the whole project benchmarks against.

use clap::ValueEnum;

/// A synthetic tree profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Profile {
    /// One directory with 100,000 empty files of mixed extensions.
    Flat100k,
    /// A 4-level tree (10 x 10 x 10 x 10 dirs) holding 1,000,000 files.
    Deep1m,
    /// One directory with 10,000 small PNG images.
    Images10k,
}

impl Profile {
    pub const ALL: [Profile; 3] = [Profile::Flat100k, Profile::Deep1m, Profile::Images10k];

    pub fn name(self) -> &'static str {
        match self {
            Profile::Flat100k => "flat100k",
            Profile::Deep1m => "deep1m",
            Profile::Images10k => "images10k",
        }
    }

    /// Number of regular files the profile contains once generated.
    pub fn file_count(self) -> u64 {
        match self {
            Profile::Flat100k => 100_000,
            Profile::Deep1m => 1_000_000,
            Profile::Images10k => 10_000,
        }
    }
}

pub fn or_all(v: Vec<Profile>) -> Vec<Profile> {
    if v.is_empty() {
        Profile::ALL.to_vec()
    } else {
        v
    }
}
