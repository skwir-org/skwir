//! About skwir (`ui.about`, in More and the palette): its icon, name,
//! version and licence, the way to its website and to the licences its
//! package installed, and what a report of a problem asks of the system
//! it runs on (`.github/ISSUE_TEMPLATE/problem.yml`), copied in one click.
//!
//! The system's own name and where the licences are installed are read on
//! a worker (`skwir_platform::about`) and the dialog opens with them, so
//! nothing in it changes once drawn. It is something to read, as F1's
//! overview is: a key meant for the window behind it closes it
//! (`Shell::run`), and Tab walks its buttons.

use std::path::PathBuf;

use gpui::{
    AnyElement, ClipboardItem, Context, FocusHandle, MouseButton, SharedString, Window, div,
    prelude::*, px,
};

use crate::controls::{self, ButtonStyle};
use crate::keymap::RunCommand;
use crate::modal::{self, FocusReturn};
use crate::shell::Shell;
use crate::theme::Theme;

/// What every package says of skwir, in the words of
/// `crates/skwir/Cargo.toml` (a test holds them to it), without the full
/// stop About draws after them.
const SUMMARY: &str = "An augmented file manager, for humans and agents";
const LICENCE: &str = env!("CARGO_PKG_LICENSE");
const WEBSITE: &str = env!("CARGO_PKG_REPOSITORY");

/// The version as `skwir --version` says it, and whether it is a build
/// for development, slower than a release.
fn version() -> String {
    let version = env!("CARGO_PKG_VERSION");
    if cfg!(debug_assertions) {
        format!("{version} (debug build)")
    } else {
        version.to_owned()
    }
}

pub struct About {
    focus: FocusHandle,
    /// Copy details, Website, Licences: what Tab walks, of those shown.
    copy: FocusHandle,
    website: FocusHandle,
    licences_button: FocusHandle,
    previous: FocusReturn,
    /// What a report says of where skwir runs: a label and its value.
    facts: Vec<(&'static str, String)>,
    /// What shows the licences this installation keeps, if it keeps any.
    licences: Option<PathBuf>,
}

impl About {
    fn buttons(&self) -> Vec<&FocusHandle> {
        let mut buttons = vec![&self.copy];
        if !WEBSITE.is_empty() {
            buttons.push(&self.website);
        }
        if self.licences.is_some() {
            buttons.push(&self.licences_button);
        }
        buttons
    }
}

/// The graphics as Vulkan names them: the device, then its driver.
fn graphics(specs: &gpui::GpuSpecs) -> String {
    let driver = format!("{} {}", specs.driver_name.trim(), specs.driver_info.trim());
    let said = [specs.device_name.trim(), driver.trim()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
    if specs.is_software_emulated {
        format!("{said} (drawn by the processor)")
    } else {
        said
    }
}

/// What Copy details puts on the clipboard: the version as `skwir
/// --version` says it, then each fact on a line.
fn details(facts: &[(&str, String)]) -> String {
    std::iter::once(format!("skwir {}", version()))
        .chain(
            facts
                .iter()
                .map(|(label, value)| format!("{label}: {value}")),
        )
        .collect::<Vec<_>>()
        .join("\n")
}

impl Shell {
    /// Open About, or close it.
    pub fn toggle_about(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(open) = self.about.take() {
            open.previous.restore(window, &self.ui().focus);
            cx.notify();
            return;
        }
        self.off_thread(
            "about",
            || {
                (
                    skwir_platform::about::system_name(),
                    skwir_platform::about::licences(),
                )
            },
            |this, (system, licences), window, cx| {
                // Something else took the keyboard while it was read.
                if this.modal_open() {
                    return;
                }
                let arch = std::env::consts::ARCH;
                let system = system.unwrap_or_else(|| skwir_core::system::NAME.to_owned());
                let mut facts = vec![("System", format!("{system} ({arch})"))];
                let display = cx.compositor_name();
                if !display.is_empty() {
                    facts.push(("Display", display.to_owned()));
                }
                if let Some(specs) = window.gpu_specs() {
                    facts.push(("Graphics", graphics(&specs)));
                }
                let previous = FocusReturn::capture(window, cx);
                let focus = cx.focus_handle();
                window.focus(&focus);
                this.about = Some(About {
                    focus,
                    copy: cx.focus_handle(),
                    website: cx.focus_handle(),
                    licences_button: cx.focus_handle(),
                    previous,
                    facts,
                    licences,
                });
                cx.notify();
            },
            window,
            cx,
        );
    }

    /// Tab and Shift+Tab: the next or the previous of its buttons, round
    /// their ends.
    fn about_tab(&mut self, delta: isize, window: &mut Window) {
        let Some(about) = &self.about else { return };
        let buttons = about.buttons();
        let len = buttons.len().cast_signed();
        let next = match buttons.iter().position(|b| b.is_focused(window)) {
            Some(at) => (at.cast_signed() + delta).rem_euclid(len),
            None if delta > 0 => 0,
            None => len - 1,
        };
        window.focus(buttons[next.cast_unsigned()]);
    }

    fn copy_about(&mut self, cx: &mut Context<Self>) {
        let Some(about) = &self.about else { return };
        cx.write_to_clipboard(ClipboardItem::new_string(details(&about.facts)));
        self.set_status("Copied skwir’s version and what it runs on", false, cx);
    }

    /// The licences in a tab of their own, the third-party notices
    /// selected where the package keeps them.
    fn show_licences(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.about.as_ref().and_then(|a| a.licences.clone()) else {
            return;
        };
        self.toggle_about(window, cx);
        let at = crate::links::RevealAt {
            tab: true,
            line: None,
        };
        self.reveal_path_at(self.pane, &path, at, window, cx);
    }
}

/// Drawn before the toasts, not deferred: what Copy details answers is
/// read above it.
pub fn render(shell: &Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let about = shell.about.as_ref()?;
    let theme = cx.global::<Theme>().clone();
    let mut facts = div()
        .flex()
        .flex_col()
        .gap_1()
        .p_3()
        .rounded_md()
        .bg(theme.code_bg);
    for (row, (label, value)) in about.facts.iter().enumerate() {
        facts = facts.child(
            div()
                .flex()
                .gap_3()
                .child(
                    div()
                        .w(px(64.))
                        .flex_none()
                        .text_color(theme.text_muted)
                        .child(*label),
                )
                .child(crate::elements::clip::clipped(
                    div().id(("about-fact", row)).flex_1().min_w_0().truncate(),
                    value.clone(),
                    shell.font.clone(),
                )),
        );
    }
    let mut buttons = div().flex().justify_end().gap_2().child(
        controls::button(
            "about-copy",
            "Copy details",
            Some("copy"),
            ButtonStyle::Plain,
            &about.copy,
            &theme,
        )
        .on_click(cx.listener(|this, _, _, cx| this.copy_about(cx))),
    );
    if !WEBSITE.is_empty() {
        let font = shell.font.clone();
        buttons = buttons.child(
            controls::button(
                "about-website",
                "Website",
                Some("external"),
                ButtonStyle::Plain,
                &about.website,
                &theme,
            )
            .tooltip(move |_, cx| crate::toolbar::tooltip(WEBSITE.into(), font.clone(), cx))
            .on_click(cx.listener(|this, _, _, cx| this.open_address(WEBSITE, cx))),
        );
    }
    if about.licences.is_some() {
        buttons = buttons.child(
            controls::button(
                "about-licences",
                "Licences",
                Some("folder"),
                ButtonStyle::Plain,
                &about.licences_button,
                &theme,
            )
            .on_click(cx.listener(|this, _, window, cx| this.show_licences(window, cx))),
        );
    }
    let panel = modal::panel(460., &theme)
        .p_5()
        .gap_4()
        .line_height(px(20.))
        .track_focus(&about.focus)
        .key_context("About")
        .on_action(cx.listener(
            |this, action: &RunCommand, window, cx| match action.name.as_str() {
                "ui.toolbar_next" => this.about_tab(1, window),
                "ui.toolbar_prev" => this.about_tab(-1, window),
                _ => cx.propagate(),
            },
        ))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .child(crate::icons::app_icon(48.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(div().text_xl().child("skwir"))
                        .child(div().text_color(theme.text_muted).child(SharedString::from(
                            format!("Version {} · {LICENCE}", version()),
                        ))),
                ),
        )
        .child(SharedString::from(format!("{SUMMARY}.")))
        .child(facts)
        .child(buttons);
    Some(
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .occlude()
            .bg(theme.scrim)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.toggle_about(window, cx)),
            )
            .child(panel)
            .into_any_element(),
    )
}

#[cfg(test)]
mod tests {
    use super::{SUMMARY, details, graphics};

    #[test]
    fn about_and_every_package_say_skwir_in_the_same_words() {
        // The .deb, the .rpm and Nix read the binary's manifest; the AUR's
        // files cannot.
        let manifest = include_str!("../../skwir/Cargo.toml");
        let package = manifest
            .lines()
            .find_map(|line| line.strip_prefix("description = \"")?.strip_suffix('"'));
        assert_eq!(package, Some(SUMMARY), "crates/skwir/Cargo.toml");
        let pkgbuild = include_str!("../../../tools/package/aur/PKGBUILD");
        assert!(
            pkgbuild
                .lines()
                .any(|line| line == format!("pkgdesc='{SUMMARY}'")),
            "tools/package/aur/PKGBUILD"
        );
        let srcinfo = include_str!("../../../tools/package/aur/.SRCINFO");
        assert!(
            srcinfo
                .lines()
                .any(|line| line == format!("\tpkgdesc = {SUMMARY}")),
            "tools/package/aur/.SRCINFO"
        );
    }

    #[test]
    fn the_graphics_say_the_device_then_its_driver() {
        let specs = |device: &str, driver: &str, info: &str, software| gpui::GpuSpecs {
            is_software_emulated: software,
            device_name: device.into(),
            driver_name: driver.into(),
            driver_info: info.into(),
        };
        assert_eq!(
            graphics(&specs("Radeon 780M", "radv", "Mesa 25.1", false)),
            "Radeon 780M · radv Mesa 25.1"
        );
        assert_eq!(
            graphics(&specs("llvmpipe", "llvmpipe", "", true)),
            "llvmpipe · llvmpipe (drawn by the processor)"
        );
        assert_eq!(graphics(&specs("", "", "", false)), "");
    }

    #[test]
    fn the_details_start_with_the_version_as_the_command_line_says_it() {
        let text = details(&[
            ("System", "Fedora Linux 42 (x86_64)".into()),
            ("Display", "Wayland".into()),
        ]);
        let mut lines = text.lines();
        assert!(
            lines
                .next()
                .unwrap()
                .starts_with(&format!("skwir {}", env!("CARGO_PKG_VERSION")))
        );
        assert_eq!(lines.next(), Some("System: Fedora Linux 42 (x86_64)"));
        assert_eq!(lines.next(), Some("Display: Wayland"));
        assert_eq!(lines.next(), None);
    }
}
