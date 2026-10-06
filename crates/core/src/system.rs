//! The system bibli runs on, by name, and what is not written for it yet.
//!
//! bibli is built for Linux, macOS and Windows (ADR-035). Code that has no
//! implementation on a system yet answers with [`not_yet`], an error that
//! names the system, never with Linux's behaviour where it would be wrong.

/// The system's name as the user knows it.
pub const NAME: &str = if cfg!(target_os = "linux") {
    "Linux"
} else if cfg!(target_os = "macos") {
    "macOS"
} else if cfg!(windows) {
    "Windows"
} else {
    "this system"
};

/// `what` ("Restoring from the trash") has no implementation on this
/// system yet: an unsupported error saying so, in words for the user.
pub fn not_yet(what: &str) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        format!("{what} is not available on {NAME} yet"),
    )
}

/// For tests: whether they run under Wine (`just test-windows`), which
/// lacks `what` that Windows has (its shell trashes nothing). A test
/// that needs it then says so and passes; it runs whole on Windows.
#[doc(hidden)]
pub fn under_wine(what: &str) -> bool {
    let wine = std::env::var_os("BIBLI_TEST_UNDER_WINE").is_some();
    if wine {
        eprintln!("skipped under Wine, which lacks {what}");
    }
    wine
}
