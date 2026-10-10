//! What skwir reads of bat, the pager whose grammars and themes color
//! code in the preview: the folder of the themes its user added, so code
//! reads in skwir in the colors it has in the terminal.
use std::ffi::OsString;
use std::path::PathBuf;

/// The folder bat reads its user's themes from (`bat cache --build`):
/// `themes` in `$BAT_CONFIG_DIR`, else in `bat` of the user's
/// configuration, `~/.config/bat` on Linux and on macOS alike (bat reads
/// `$XDG_CONFIG_HOME` there too), `%APPDATA%\bat` on Windows. It may not
/// exist.
pub fn themes_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    let base = dirs::config_dir();
    #[cfg(not(windows))]
    let base = skwir_core::xdg::config_home();
    config_dir(std::env::var_os("BAT_CONFIG_DIR"), base).map(|dir| dir.join("themes"))
}

/// bat's folder, from what `$BAT_CONFIG_DIR` says and the user's
/// configuration folder. A relative path in the variable is not read
/// against skwir's working folder: it is ignored.
fn config_dir(variable: Option<OsString>, base: Option<PathBuf>) -> Option<PathBuf> {
    variable
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .or_else(|| base.map(|base| base.join("bat")))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn the_variable_names_bat_s_folder_and_the_configuration_holds_it_otherwise() {
        let base = Some(PathBuf::from("/home/someone/.config"));
        assert_eq!(
            config_dir(Some("/opt/bat-config".into()), base.clone()),
            Some(PathBuf::from("/opt/bat-config"))
        );
        assert_eq!(
            config_dir(None, base.clone()),
            Some(PathBuf::from("/home/someone/.config/bat"))
        );
        assert_eq!(
            config_dir(Some("relative/bat".into()), base),
            Some(PathBuf::from("/home/someone/.config/bat"))
        );
        assert_eq!(config_dir(None, None), None);
    }
}
