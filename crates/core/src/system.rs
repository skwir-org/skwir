//! The system skwir runs on, by name, and what is not written for it yet.
//!
//! skwir is built for Linux, macOS and Windows (ADR-035). Code that has no
//! implementation on a system yet answers with [`not_yet_words`], an
//! error that names the system, never with Linux's behaviour where it
//! would be wrong.

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

/// What has no implementation on this system yet, said whole
/// (`words!(…_not_yet = "Printing is not available on { $system } yet",
/// system = system::named())`): an unsupported error holding its words,
/// said in the reader's language (`op::io_words` finds them).
pub fn not_yet_words(words: impl Into<crate::words::Words>) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::Unsupported, words.into())
}

/// The system's name, for a message's argument: [`NAME`] as it is, or
/// words for a system Skwir does not name.
pub fn named() -> crate::words::Words {
    if cfg!(any(target_os = "linux", target_os = "macos", windows)) {
        crate::words::Words::as_written(NAME)
    } else {
        crate::words!(
            system_this = "this system",
            note = "Said where a system's name goes, for a system Skwir does not name: Printing is not available on this system yet."
        )
    }
}

/// For tests: whether they run under Wine (`just test-windows`), which
/// lacks `what` that Windows has (its shell trashes nothing). A test
/// that needs it then says so and passes; it runs whole on Windows.
#[doc(hidden)]
pub fn under_wine(what: &str) -> bool {
    let wine = std::env::var_os("SKWIR_TEST_UNDER_WINE").is_some();
    if wine {
        eprintln!("skipped under Wine, which lacks {what}");
    }
    wine
}
