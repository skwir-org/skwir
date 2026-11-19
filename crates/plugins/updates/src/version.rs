//! A release's version as the site's file and Cargo write it:
//! `MAJOR.MINOR.PATCH`, three numbers and nothing else (ADR-142, 6).

use std::fmt;

/// A version, ordered as releases follow each other.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version(pub u64, pub u64, pub u64);

impl Version {
    /// `text` read as three numbers separated by dots, each of digits
    /// alone; none for anything else (a pre-release, a `v` before it,
    /// spaces, a fourth part).
    pub fn parse(text: &str) -> Option<Version> {
        let mut parts = text.split('.');
        let mut number = || {
            let part = parts.next()?;
            (!part.is_empty() && part.len() <= 9 && part.bytes().all(|b| b.is_ascii_digit()))
                .then(|| part.parse().ok())
                .flatten()
        };
        let version = Version(number()?, number()?, number()?);
        parts.next().is_none().then_some(version)
    }

    /// The version of this build of the plugin, which is skwir's.
    pub fn running() -> Option<Version> {
        Version::parse(env!("CARGO_PKG_VERSION"))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

#[cfg(test)]
mod tests {
    use super::Version;

    #[test]
    fn only_three_numbers_are_a_version() {
        assert_eq!(Version::parse("0.6.2"), Some(Version(0, 6, 2)));
        assert_eq!(Version::parse("10.0.11"), Some(Version(10, 0, 11)));
        for text in [
            "",
            "0.6",
            "0.6.2.1",
            "v0.6.2",
            "0.6.2-rc.1",
            " 0.6.2",
            "0.6.2\n",
            "0..2",
            "0.6.x",
            "+1.0.0",
            "1.0.0000000000",
        ] {
            assert_eq!(Version::parse(text), None, "{text:?}");
        }
        assert_eq!(Version(0, 6, 2).to_string(), "0.6.2");
        assert!(Version::running().is_some(), "Cargo's version reads");
    }

    #[test]
    fn versions_follow_each_other_by_their_numbers() {
        let parse = |text| Version::parse(text).unwrap();
        assert!(parse("0.10.0") > parse("0.9.9"));
        assert!(parse("1.0.0") > parse("0.99.99"));
        assert!(parse("0.5.10") > parse("0.5.9"));
        assert_eq!(parse("0.5.1"), Version(0, 5, 1));
    }
}
