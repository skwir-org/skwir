//! The Settings dialog: a self-contained view over the core's settings schema,
//! plugin host and key hints. It holds no reference to the shell, so it can be
//! hosted in an overlay or in a window of its own.
//!
//! Changes apply immediately. Switches and choices submit on click; text and
//! number fields submit on Enter or when they lose focus. Every change goes
//! through `settings.apply`, which saves atomically on a worker; while one save
//! is in flight later changes wait in `queue` and leave as one batch. A failed
//! save restores the live value next to the control and says why.
//!
//! Rendering lives in `settings_view.rs`.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use gpui::{
    AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable, Subscription, Window,
};
use skwir_core::App;
use skwir_core::command::{Invocation, Outcome};
use skwir_core::plugin::{PluginInfo, RequirementStatus};
use skwir_core::settings::{SaveState, SettingKind, SettingSpec, page};
use std::sync::Mutex;

use crate::elements::popover::PopoverAnchor;
use crate::input::{InputEvent, TextInput};
use crate::keymap::Bound;

/// Settings namespaces that are not plugins, each a page of the dialog:
/// what skwir does with files, how it looks, and how the preview reads.
/// A preference goes where its subject is: the plugin whose content it
/// concerns has it on its page (an engine, the player of audio and video);
/// one the shell applies to several plugins' content is a namespace of the
/// shell's own, as `reading` is for the reader and reading mode, and the
/// colors code is drawn in, shared by documents, books, comics, Markdown
/// and the code view. `ui` is only how skwir looks.
pub const CORE_OWNER: &str = "core";
pub const UI_OWNER: &str = "ui";
pub const READING_OWNER: &str = "reading";
/// The stores notes are kept in (ADR-027), shared by several plugins.
pub const NOTES_OWNER: &str = skwir_core::notes::SETTINGS;
/// What leaves skwir (ADR-044): the private folders.
pub const PRIVACY_OWNER: &str = skwir_core::permissions::SETTINGS;

pub const THEME_KEY: &str = "ui.theme";
/// Whether skwir wears the accent the desktop's settings name (ADR-071,
/// `desktop_style`), and its fonts where a font says Automatic.
pub const DESKTOP_COLORS_KEY: &str = "ui.desktop_colors";
/// The fonts skwir writes in, by role (ADR-073, `fonts.rs`): the
/// interface's, what is read, code. Each is Automatic or a family.
pub const FONT_KEY: &str = "ui.font";
pub const READING_FONT_KEY: &str = "reading.font";
pub const CODE_FONT_KEY: &str = "ui.code_font";
/// The three, in that order.
pub const FONT_KEYS: [&str; 3] = [FONT_KEY, READING_FONT_KEY, CODE_FONT_KEY];
/// The fonts' Automatic.
pub const FONT_AUTO: &str = "auto";
/// The icon theme skwir draws with, by its folder's name; Automatic is the
/// desktop's (`icons::IconCache::set_theme`). The free desktops' only:
/// macOS and Windows draw the system's own icons.
pub const ICON_THEME_KEY: &str = "ui.icon_theme";
/// `ui.icon_theme`'s Automatic.
pub const ICON_THEME_AUTO: &str = "auto";
pub const DATES_KEY: &str = "ui.date_format";
/// Whether the grid crops thumbnails to fill their box.
pub const GRID_FILL_KEY: &str = "ui.grid_fill";
/// How a drag of several files is drawn (`dragicon::Layout`).
pub const DRAG_IMAGE_KEY: &str = "ui.drag_image";
/// The dialog's pages unfolded to show all their settings (ADR-072), by
/// their words: the window's state, kept until folded again.
pub const UNFOLDED_KEY: &str = "ui.settings_unfolded";
/// The views given a button beside the address, or whose button was
/// taken away (`address::ViewButtons`): made from the buttons' own menu.
pub const VIEW_BUTTONS_KEY: &str = "ui.view_buttons";
/// Whether a folder has the Narrow by bar above its files (`narrow.rs`).
pub const NARROW_FOLDERS_KEY: &str = "ui.narrow_folders";
/// Whether the tab strip shows for a window that has only one tab.
pub const SINGLE_TAB_KEY: &str = "ui.single_tab";
/// Whether a folder's trail starts at the root rather than at the
/// sidebar's folder that holds it (`Shell::sync_roots`).
pub const FULL_PATH_KEY: &str = "ui.full_path";
/// How documents are read: one scrolling column, or a page at a time.
pub const READER_CONTINUOUS_KEY: &str = "reading.continuous";
/// How wide a reading column may grow (`markdown::measure`).
pub const READING_WIDTH_KEY: &str = "reading.width";
/// Whether a book's position counts its printed pages (`ui.printed_pages`).
pub const PRINTED_PAGES_KEY: &str = "reading.printed_pages";
/// Whether the contents stay open beside what is read (`ui.contents_pin`).
pub const CONTENTS_PINNED_KEY: &str = "reading.contents_pinned";
/// Whether a text's long lines wrap to the panel (`ui.wrap_lines`).
pub const WRAP_LINES_KEY: &str = "reading.wrap_lines";
/// Whether a 3D model is drawn in perspective (`ui.model_perspective`).
pub const MODEL_PERSPECTIVE_KEY: &str = "reading.model_perspective";
/// Whether a 3D model is shaded smoothly (`ui.model_smooth`).
pub const MODEL_SMOOTH_KEY: &str = "reading.model_smooth";
/// How long a slideshow shows each picture, in seconds
/// (`fullscreen.rs`), chosen here or from its bar.
pub const SLIDESHOW_KEY: &str = "reading.slideshow";
/// Whether the picture the next step shows is decoded ahead
/// (`Shell::read_ahead_next`).
pub const READ_AHEAD_KEY: &str = "reading.read_ahead";
/// The paces a slideshow goes at: the setting's choices and its bar's.
pub const SLIDESHOW_PACES: [(&str, &str); 4] = [
    ("3", "3 seconds"),
    ("5", "5 seconds"),
    ("10", "10 seconds"),
    ("20", "20 seconds"),
];
/// Whether code is drawn light or dark: Automatic is as skwir is
/// (`code_preview::CodeTheme::chosen`).
pub const CODE_APPEARANCE_KEY: &str = "reading.code_appearance";
/// The colors code is drawn in when it is light, and when it is dark
/// (`code_preview::CodeTheme`).
pub const CODE_LIGHT_KEY: &str = "reading.code_light";
pub const CODE_DARK_KEY: &str = "reading.code_dark";
/// The player's preferences. The audio and video plugin (`media`)
/// declares them, with the rest of what concerns playback; the shell's
/// player reads them. With the plugin off there is no player, and they are
/// gone with it. `crates/skwir/tests/settings.rs` holds the two to the
/// same names and choices.
/// Whether a track that starts to play keeps playing while you browse.
pub const KEEP_PLAYING_KEY: &str = "media.keep_playing";
/// Whether audio and video start to play as soon as they are selected.
pub const AUTOPLAY_KEY: &str = "media.autoplay";
/// What follows a track that ends (`player::Continue`).
pub const CONTINUE_KEY: &str = "media.continue";
/// What the player does with nothing stored, as the plugin must declare it.
pub const KEEP_PLAYING_DEFAULT: bool = true;
pub const AUTOPLAY_DEFAULT: bool = false;
/// The values of `CONTINUE_KEY`, the first the default.
pub const CONTINUE_CHOICES: [&str; 3] = [
    crate::player::Continue::Stop.key(),
    crate::player::Continue::Next.key(),
    crate::player::Continue::Shuffle.key(),
];

/// The icon theme setting: Automatic, then the themes installed, by name,
/// once listed (`list_icon_themes`: it reads the icon folders, a worker's
/// work), whose value is the theme's folder (`breeze`). The one chosen is
/// always a choice, said missing once the list shows it is not installed.
pub fn icon_theme_spec(
    chosen: &str,
    themes: Option<&[skwir_platform::icons::Theme]>,
) -> SettingSpec {
    let mut choices = vec![(ICON_THEME_AUTO.to_owned(), "Automatic".to_owned())];
    if let Some(themes) = themes {
        choices.extend(themes.iter().map(|t| (t.folder.clone(), t.name.clone())));
    }
    if !chosen.is_empty() && !choices.iter().any(|(value, _)| value == chosen) {
        let label = if themes.is_some() {
            format!("{chosen} (not installed)")
        } else {
            chosen.to_owned()
        };
        choices.push((chosen.to_owned(), label));
    }
    let pairs: Vec<(&str, &str)> = choices
        .iter()
        .map(|(value, label)| (value.as_str(), label.as_str()))
        .collect();
    SettingSpec::choice(
        ICON_THEME_KEY,
        "Icon theme",
        "The icons skwir draws files, folders and places with. Automatic follows your desktop's choice.",
        ICON_THEME_AUTO,
        &pairs,
    )
    .more_choices("the icon themes installed")
    .essential()
}

/// A font setting (`FONT_KEY`, `READING_FONT_KEY`, `CODE_FONT_KEY`):
/// Automatic, then the families offered for it once listed
/// (`list_fonts`: the text system's index and a program, a worker's
/// work). The one chosen is always a choice, said missing once the list
/// shows it is not installed.
pub fn font_spec(
    key: &str,
    chosen: &str,
    installed: Option<&crate::fonts::Installed>,
) -> SettingSpec {
    let mut choices = vec![(FONT_AUTO.to_owned(), "Automatic".to_owned())];
    if let Some(installed) = installed {
        choices.extend(
            installed
                .offered(key)
                .into_iter()
                .map(|family| (family.to_owned(), family.to_owned())),
        );
    }
    if !chosen.is_empty() && !choices.iter().any(|(value, _)| value == chosen) {
        let label = match installed {
            Some(installed) if !installed.has(chosen) => format!("{chosen} (not installed)"),
            _ => chosen.to_owned(),
        };
        choices.push((chosen.to_owned(), label));
    }
    let pairs: Vec<(&str, &str)> = choices
        .iter()
        .map(|(value, label)| (value.as_str(), label.as_str()))
        .collect();
    let spec = |title: &str, description: &str| {
        SettingSpec::choice(key, title, description, FONT_AUTO, &pairs)
            .more_choices("the fonts installed")
    };
    match key {
        READING_FONT_KEY => spec(
            "Reading font",
            "The font books, Markdown and HTML files, documents and mail are read in. Automatic is your desktop's document font while Desktop colors is on, else the interface's.",
        )
        .section("Text")
        .essential(),
        CODE_FONT_KEY => spec(
            "Code font",
            "The font of source and text files, code in documents, the byte view and the terminal, where each letter takes the same width. Automatic is your desktop's monospace font while Desktop colors is on, else skwir's own.",
        )
        .page(page::READING)
        .section("Code"),
        _ => spec(
            "Font",
            "The font of skwir's menus, lists, bars and dialogs. Automatic is your desktop's while Desktop colors is on, else skwir's own: Inter where it is installed.",
        ),
    }
}

/// What a font setting says, Automatic when nothing.
fn font_chosen(app: &App, key: &str) -> String {
    app.settings
        .get::<String>(key)
        .unwrap_or_else(|| FONT_AUTO.to_owned())
}

/// List the families the text system draws on a worker and declare the
/// font settings again with them: at start, so a script or an agent can
/// choose one, and when the settings open, where a font installed since
/// is found.
pub(crate) fn list_fonts(app: &Arc<App>, text: Arc<gpui::TextSystem>) {
    let weak = app.weak();
    app.scheduler.spawn(
        skwir_core::scheduler::Priority::Normal,
        "fonts",
        app.lifetime().child(),
        move || {
            let installed = crate::fonts::Installed::read(&text);
            let Some(app) = weak.upgrade() else { return };
            for key in FONT_KEYS {
                app.settings
                    .declare(font_spec(key, &font_chosen(&app, key), Some(&installed)));
            }
            app.wake();
        },
    );
}

/// A look's code theme setting: skwir's own colors, bat's themes of that
/// look, then those the user added to bat once read (`list_code_themes`),
/// the one chosen always among them (`code_preview::choices`).
pub fn code_theme_spec(
    dark: bool,
    chosen: &str,
    user: Option<&[Arc<crate::code_preview::UserTheme>]>,
) -> SettingSpec {
    let choices = crate::code_preview::choices(dark, chosen, user);
    let pairs: Vec<(&str, &str)> = choices
        .iter()
        .map(|(value, label)| (value.as_str(), label.as_str()))
        .collect();
    let (key, title, look, choice) = if dark {
        (CODE_DARK_KEY, "Dark theme", "dark", "Dark")
    } else {
        (CODE_LIGHT_KEY, "Light theme", "light", "Light")
    };
    SettingSpec::choice(
        key,
        title,
        &format!(
            "The colors of source files and of code in documents when code is {look}: while skwir is {look}, or always when Light or dark above says {choice}. Default is skwir's own, on its {look} background; a theme brings its own background, bold and italic. Themes you added to bat, in its themes folder, follow bat's own."
        ),
        crate::code_preview::DEFAULT,
        &pairs,
    )
    .more_choices("the themes you added to bat")
    .section("Code")
}

/// Read the themes the user added to bat on a worker and declare the code
/// theme settings again with them: at start, and when the settings open,
/// where a theme added or edited since is read.
pub(crate) fn list_code_themes(app: &Arc<App>) {
    let weak = app.weak();
    app.scheduler.spawn(
        skwir_core::scheduler::Priority::Normal,
        "code-themes",
        app.lifetime().child(),
        move || {
            let user = crate::code_preview::user_themes(true);
            let Some(app) = weak.upgrade() else { return };
            for (dark, key) in [(false, CODE_LIGHT_KEY), (true, CODE_DARK_KEY)] {
                let chosen = app
                    .settings
                    .get::<String>(key)
                    .unwrap_or_else(|| crate::code_preview::DEFAULT.to_owned());
                app.settings
                    .declare(code_theme_spec(dark, &chosen, Some(&user)));
            }
            app.wake();
        },
    );
}

/// List the installed icon themes on a worker and declare the icon theme
/// setting again with them: when the settings open, where the list is
/// read. Nothing where the system draws its own icons.
pub(crate) fn list_icon_themes(app: &Arc<App>) {
    if skwir_platform::system_icons::AVAILABLE {
        return;
    }
    let weak = app.weak();
    app.scheduler.spawn(
        skwir_core::scheduler::Priority::Normal,
        "icon-themes",
        app.lifetime().child(),
        move || {
            let themes = skwir_platform::icons::installed_themes();
            let Some(app) = weak.upgrade() else { return };
            let chosen = app
                .settings
                .get::<String>(ICON_THEME_KEY)
                .unwrap_or_default();
            app.settings
                .declare(icon_theme_spec(&chosen, Some(&themes)));
            app.wake();
        },
    );
}

/// Declare the shell's own preferences. They live in the shell's namespaces
/// (`ui`, `reading`) and exist only while a shell does, so headless tools
/// never see them.
pub fn declare(app: &App) {
    app.settings.declare(
        SettingSpec::choice(
            THEME_KEY,
            "Theme",
            "Follow the desktop's light or dark preference, or always use one.",
            "auto",
            &[("auto", "Automatic"), ("light", "Light"), ("dark", "Dark")],
        )
        .essential(),
    );
    if skwir_platform::desktop_style::READ {
        app.settings.declare(SettingSpec::toggle(
            DESKTOP_COLORS_KEY,
            "Desktop colors",
            "Draw with the colors chosen in your desktop's settings, and write in its fonts where a font says Automatic. The colors are Plasma's color scheme, or your GTK theme's, with your desktop's accent. Off, skwir keeps its own colors and fonts. More contrast, when your desktop asks for it, is followed either way.",
            true,
        )
        .essential());
    }
    app.settings
        .declare(font_spec(FONT_KEY, &font_chosen(app, FONT_KEY), None));
    if !skwir_platform::system_icons::AVAILABLE {
        let chosen = app
            .settings
            .get::<String>(ICON_THEME_KEY)
            .unwrap_or_default();
        app.settings.declare(icon_theme_spec(&chosen, None));
        // Before anything is drawn, as the shell will say it again.
        skwir_platform::icons::choose_theme(
            Some(chosen.as_str()).filter(|c| *c != ICON_THEME_AUTO),
        );
    }
    app.settings.declare(SettingSpec::choice(
        DATES_KEY,
        "Dates",
        "Friendly dates read as Today 14:02, Tuesday 09:30 or 12 Mar, with the exact time in a tooltip. ISO shows 2026-03-12 14:02 everywhere.",
        "friendly",
        &[("friendly", "Friendly"), ("iso", "ISO 8601")],
    )
    .essential());
    app.settings.declare(
        SettingSpec::toggle(
            SINGLE_TAB_KEY,
            "Show a lone tab",
            "Show the tab strip even when the window has one tab, named like the place it shows. Off gives the lists that row; New tab stays in the More menu and on Ctrl+T.",
            false,
        )
        .page(page::GENERAL)
        .section("Tabs"),
    );
    app.settings.declare(
        SettingSpec::toggle(
            FULL_PATH_KEY,
            "Show the full path",
            "Show a folder's whole path above its files, from the root, each folder under its own name: / › home › someone › Documents › 2026. Off starts it at the deepest folder of the sidebar that holds it, under the sidebar's name: Documents › 2026, a drive's name for a folder on a drive.",
            false,
        )
        .page(page::GENERAL)
        .section("Address"),
    );
    app.settings.declare(
        SettingSpec::toggle(
            NARROW_FOLDERS_KEY,
            "Narrow folders",
            "Show the Narrow by bar above a folder's files: their kinds, the years they were modified and their tags, each with how many files, to show only those. The choice stays while you browse the folders inside. A search and a tag's folder offer it whatever this says.",
            false,
        )
        .page(page::VIEWS)
        .section("Folders")
        .essential(),
    );
    app.settings.declare(
        SettingSpec::toggle(
            GRID_FILL_KEY,
            "Fill thumbnails",
            "Crop each thumbnail in the grid to a square, so a folder of pictures reads as a gallery. Off shows every picture whole.",
            false,
        )
        .page(page::VIEWS)
        .section("Grid")
        .essential(),
    );
    // What the grid's tiles say under their names (`CAPTIONS`), in the
    // order they stand there.
    for caption in &crate::views::grid::CAPTIONS {
        app.settings.declare(
            SettingSpec::toggle(caption.setting, caption.title, caption.description, false)
                .page(page::VIEWS)
                .section("Grid"),
        );
    }
    app.settings.declare(
        SettingSpec::toggle(
            READER_CONTINUOUS_KEY,
            "Continuous scrolling",
            "Read a document, a book laid out in pages or a comic as one scrolling column. Off shows one page at a time, and the reader's own button still changes it for the document in front of you.",
            true,
        )
        .section("Pages")
        .essential(),
    );
    app.settings.declare(font_spec(
        READING_FONT_KEY,
        &font_chosen(app, READING_FONT_KEY),
        None,
    ));
    app.settings.declare(
        SettingSpec::choice(
            READING_WIDTH_KEY,
            "Reading width",
            "How long the lines of a book, a Markdown or HTML file may grow before the text stops widening and sits in the middle of the preview. Narrow is about 65 characters, Comfortable 85, Wide 110; Full width uses the whole panel, with small margins.",
            "comfortable",
            &[
                ("narrow", "Narrow"),
                ("comfortable", "Comfortable"),
                ("wide", "Wide"),
                ("full", "Full width"),
            ],
        )
        .section("Text")
        .essential(),
    );
    app.settings.declare(
        SettingSpec::toggle(
            PRINTED_PAGES_KEY,
            "Printed page numbers",
            "Count where you are by the page numbers a book's printed edition lists, or a PDF's pages are printed with. That is page 47 of 312 rather than chapter 3 of 12; a book without them counts chapters.",
            false,
        )
        .section("Pages"),
    );
    app.settings.declare(
        SettingSpec::toggle(
            CONTENTS_PINNED_KEY,
            "Keep contents open",
            "Keep a document's or a book's contents open beside it while you read, the text or the pages narrowed to make room. Off, they open over what you read and close once you choose a part. The pin at the top of the contents changes it too.",
            false,
        )
        .section("Pages"),
    );
    app.settings.declare(
        SettingSpec::toggle(
            WRAP_LINES_KEY,
            "Wrap lines",
            "Wrap a text file's long lines to the panel, a line's number on its first row. Off, they go on sideways and the panel scrolls with them.",
            true,
        )
        .section("Text"),
    );
    app.settings.declare(
        SettingSpec::choice(
            CODE_APPEARANCE_KEY,
            "Light or dark",
            "Draw source files and code in documents light or dark. Automatic follows skwir's theme; Light or Dark keeps code in the theme chosen below for that look, with its gutter, selection and background, whatever skwir is.",
            "auto",
            &[("auto", "Automatic"), ("light", "Light"), ("dark", "Dark")],
        )
        .section("Code")
        .essential(),
    );
    for (dark, key) in [(false, CODE_LIGHT_KEY), (true, CODE_DARK_KEY)] {
        // Nothing stored and nothing declared yet: the default is chosen.
        let chosen = app
            .settings
            .get::<String>(key)
            .unwrap_or_else(|| crate::code_preview::DEFAULT.to_owned());
        app.settings.declare(code_theme_spec(dark, &chosen, None));
    }
    app.settings.declare(font_spec(
        CODE_FONT_KEY,
        &font_chosen(app, CODE_FONT_KEY),
        None,
    ));
    app.settings.declare(
        SettingSpec::choice(
            SLIDESHOW_KEY,
            "Slideshow",
            "How long a slideshow shows each picture. A slideshow plays the pictures of the folder or search on the whole screen, in the order they are listed, from the one shown to the last.",
            "5",
            &SLIDESHOW_PACES,
        )
        .page(page::MEDIA)
        .section("Pictures"),
    );
    app.settings.declare(
        SettingSpec::toggle(
            READ_AHEAD_KEY,
            "Read ahead",
            "While you step through pictures, in the preview panel, in peek or on the whole screen, decode the next one before you reach it, so it shows at once. The preview reads ahead in the panel once two steps went the same way, and holds one more picture in memory meanwhile. Off, each picture is decoded when you reach it.",
            true,
        )
        .page(page::MEDIA)
        .section("Pictures"),
    );
    app.settings.declare(
        SettingSpec::toggle(
            MODEL_PERSPECTIVE_KEY,
            "Perspective",
            "Draw 3D models in perspective, nearer parts larger, as a camera sees them. Off, every part keeps its size at any depth, as in the model's picture and in a drawing.",
            false,
        )
        .page(page::MEDIA)
        .section("3D models"),
    );
    app.settings.declare(
        SettingSpec::toggle(
            MODEL_SMOOTH_KEY,
            "Smooth shading",
            "Shade 3D models smoothly across their faces, as a scanned or sculpted surface reads best. Off, each face is shaded flat, so a printed part's facets and edges show.",
            false,
        )
        .page(page::MEDIA)
        .section("3D models"),
    );
    app.settings.declare(SettingSpec::state(
        UNFOLDED_KEY,
        "The settings pages unfolded to show all their settings",
        toml::Value::Array(Vec::new()),
    ));
    app.settings.declare(SettingSpec::data(
        VIEW_BUTTONS_KEY,
        "The views given a button beside the address (true) or whose button was taken away (false), by id; any other has a button if it shows the files as they are listed (list, grid, columns)",
        toml::Value::Table(toml::Table::new()),
    ));
    app.settings.declare(SettingSpec::state(
        crate::shortcuts::HINTS_KEY,
        "Commands whose shortcut hint was already shown",
        toml::Value::Array(Vec::new()),
    ));
    // The list's detail columns, in the order they stand (`COLUMNS`).
    for column in &crate::views::list::COLUMNS {
        app.settings.declare(
            SettingSpec::toggle(
                column.setting,
                column.label,
                column.description,
                column.shown,
            )
            .page(page::VIEWS)
            .section("List columns"),
        );
    }
    app.settings.declare(
        SettingSpec::choice(
            DRAG_IMAGE_KEY,
            "Dragged files",
            "How several files look while you drag them. Fan holds the one you took in front of two more leaning out behind it; Pile lays two more under it, each a little lower; Mosaic sets up to four side by side, the last square saying how many more; Single shows the one you took alone. A badge counts them, but in the mosaic. One file is always its own picture.",
            "fan",
            &[
                ("fan", "Fan"),
                ("pile", "Pile"),
                ("mosaic", "Mosaic"),
                ("single", "Single"),
            ],
        )
        .page(page::GENERAL)
        .section("Drag and drop"),
    );
}

/// Save a preference changed where it is used (the reader's button, the
/// player's menu) rather than in the dialog. It goes the dialog's way,
/// through `settings.apply`, which saves on a worker and tells everyone
/// else. The core takes one save at a time: while another runs, the change
/// waits (`flush_remembered`, on every wake) instead of failing, and
/// `preference` reads it meanwhile.
pub fn remember(
    shell: &mut crate::shell::Shell,
    key: impl Into<String>,
    value: String,
    cx: &mut gpui::Context<crate::shell::Shell>,
) {
    shell.unsaved.insert(key.into(), value);
    flush_remembered(shell, cx);
}

/// Send the waiting changes as one batch, unless a save is running.
pub fn flush_remembered(
    shell: &mut crate::shell::Shell,
    cx: &mut gpui::Context<crate::shell::Shell>,
) {
    if shell.unsaved.is_empty() || shell.app.settings.save_status().1 == SaveState::Saving {
        return;
    }
    let changes: serde_json::Map<String, serde_json::Value> = std::mem::take(&mut shell.unsaved)
        .into_iter()
        .filter(|(key, value)| shell.app.settings.text(key).as_ref() != Some(value))
        .map(|(key, value)| (key, value.into()))
        .collect();
    if changes.is_empty() {
        return;
    }
    if let Err(error) = shell.app.dispatch(&Invocation::with_args(
        "settings.apply",
        &serde_json::json!({ "changes": changes }),
    )) {
        shell.set_status(format!("{error:#}"), true, cx);
    }
}

/// A preference as it stands: the change waiting to be saved, else the
/// stored value, as text.
pub fn preference(shell: &crate::shell::Shell, key: &str) -> Option<String> {
    shell
        .unsaved
        .get(key)
        .cloned()
        .or_else(|| shell.app.settings.text(key))
}

/// Record how the user chose to read documents: the reader's button is the
/// same choice as the one in the dialog.
pub fn remember_reader_mode(
    shell: &mut crate::shell::Shell,
    continuous: bool,
    cx: &mut gpui::Context<crate::shell::Shell>,
) {
    remember(shell, READER_CONTINUOUS_KEY, continuous.to_string(), cx);
}

/// How the next document opens: the choice waiting to be saved, else the
/// stored one.
pub fn reader_mode(shell: &crate::shell::Shell) -> bool {
    preference(shell, READER_CONTINUOUS_KEY).is_none_or(|mode| mode != "false")
}

/// How long the slideshow shows each picture: the choice waiting to be
/// saved, else the stored one.
pub fn slideshow_seconds(shell: &crate::shell::Shell) -> u64 {
    preference(shell, SLIDESHOW_KEY)
        .and_then(|seconds| seconds.parse().ok())
        .filter(|seconds| (1..=3600).contains(seconds))
        .unwrap_or(5)
}

/// Settings by page (ADR-072), the pages in the sidebar's order; on each,
/// what has no section first, then the sections the page lists, in its
/// order, then the others where their first setting was declared; in a
/// section, the settings in the order they were declared
/// (`SettingSpec::order`).
pub fn page_order(specs: &mut [SettingSpec]) {
    let mut sections: HashMap<(Page, String), u64> = HashMap::new();
    for spec in specs.iter() {
        sections
            .entry((Page::of_spec(spec), spec.section.clone()))
            .and_modify(|first| *first = (*first).min(spec.order))
            .or_insert(spec.order);
    }
    specs.sort_by_cached_key(|spec| {
        let page = Page::of_spec(spec);
        let (place, listed) = BUILTIN
            .iter()
            .position(|b| b.page == page)
            .map_or((BUILTIN.len(), &[][..]), |at| (at, BUILTIN[at].sections));
        // A plugin's own page, by its id.
        let plugin = match &page {
            Page::Plugin(id) => id.clone(),
            _ => String::new(),
        };
        let section = listed
            .iter()
            .position(|section| *section == spec.section)
            .map_or_else(
                || (1, sections[&(page.clone(), spec.section.clone())]),
                |at| (0, at as u64),
            );
        (place, plugin, !spec.section.is_empty(), section, spec.order)
    });
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Page {
    General,
    Appearance,
    Views,
    Notes,
    Reading,
    Media,
    Terminal,
    Network,
    Privacy,
    Plugins,
    Keyboard,
    Changed,
    Plugin(String),
}

/// How a page draws its settings (ADR-072): each section under its
/// caption, or consecutive sections made of the same settings (titles and
/// kinds, in order) as one table, a line a section, a column a setting.
/// Indices into the settings given, which stand in their page's order.
#[derive(Debug, PartialEq, Eq)]
pub enum Block {
    Section {
        title: String,
        rows: Vec<usize>,
    },
    Table {
        columns: Vec<(String, SettingKind)>,
        lines: Vec<(String, Vec<usize>)>,
    },
}

/// The blocks of a page's settings, as `Block` says.
pub fn blocks(specs: &[&SettingSpec]) -> Vec<Block> {
    let mut runs: Vec<(String, Vec<usize>)> = Vec::new();
    for (at, spec) in specs.iter().enumerate() {
        match runs.last_mut() {
            Some((section, rows)) if *section == spec.section => rows.push(at),
            _ => runs.push((spec.section.clone(), vec![at])),
        }
    }
    // What a line is made of; nothing for what a table cannot hold: a
    // section without a title, a list of records (its records are rows of
    // their own), a setting under a switch.
    let shape = |(section, rows): &(String, Vec<usize>)| -> Option<Vec<(String, SettingKind)>> {
        if section.is_empty() {
            return None;
        }
        rows.iter()
            .map(|&at| {
                let spec = specs[at];
                (spec.kind != SettingKind::Records && spec.under.is_empty())
                    .then(|| (spec.title.clone(), spec.kind))
            })
            .collect()
    };
    let mut blocks = Vec::new();
    let mut at = 0;
    while at < runs.len() {
        let first = shape(&runs[at]);
        let mut end = at + 1;
        while first.is_some() && end < runs.len() && shape(&runs[end]) == first {
            end += 1;
        }
        match first {
            Some(columns) if end - at >= 2 => blocks.push(Block::Table {
                columns,
                lines: runs[at..end].to_vec(),
            }),
            _ => {
                let (title, rows) = runs[at].clone();
                blocks.push(Block::Section { title, rows });
            }
        }
        at = end;
    }
    blocks
}

/// The first sentence of a description, which its row shows, and whether
/// more follows: the whole description is the row's tooltip and what a
/// screen reader hears (ADR-072).
pub fn first_sentence(text: &str) -> (&str, bool) {
    for (at, _) in text.match_indices(". ") {
        let next = &text[at + 2..];
        if next.starts_with(|c: char| c.is_uppercase() || c.is_ascii_digit())
            || next.starts_with("skwir")
        {
            return (&text[..=at], true);
        }
    }
    (text, false)
}

/// One of the dialog's own pages: the word `ui.settings` and a setting
/// name it by (`skwir_core::settings::page`), the namespace whose
/// settings it shows when they name no page, its title, glyph and
/// introduction, and the order of its sections. Every reading of a page
/// (by setting, by owner, by name, its title, glyph and words, the
/// sidebar's order) is `BUILTIN`'s.
pub(crate) struct Builtin {
    page: Page,
    name: &'static str,
    owner: Option<&'static str>,
    pub(crate) title: &'static str,
    pub(crate) icon: &'static str,
    pub(crate) intro: &'static str,
    /// Listed only while a setting is on it.
    when_used: bool,
    /// Its sections in the order they stand, before any it does not list.
    sections: &'static [&'static str],
}

/// The dialog's own pages, by subject, in the sidebar's order; plugins'
/// follow.
static BUILTIN: [Builtin; 12] = [
    Builtin {
        page: Page::General,
        name: page::GENERAL,
        owner: Some(CORE_OWNER),
        title: "General",
        icon: "sliders",
        intro: "What skwir does at startup, with tabs, the split and the address, and with what you drag and drop. Changes apply as you make them.",
        when_used: false,
        sections: &["Startup", "Tabs", "Split", "Address", "Drag and drop"],
    },
    Builtin {
        page: Page::Appearance,
        name: page::APPEARANCE,
        owner: Some(UI_OWNER),
        title: "Appearance",
        icon: "appearance",
        intro: "How skwir looks.",
        when_used: false,
        sections: &[],
    },
    Builtin {
        page: Page::Views,
        name: page::VIEWS,
        owner: None,
        title: "Folders and views",
        icon: "grid",
        intro: "How a folder shows its files: the Narrow by bar above them, the grid, the list's columns, and the thumbnails drawn for them.",
        when_used: true,
        sections: &["Folders", "Grid", "List columns", "Thumbnails"],
    },
    Builtin {
        page: Page::Notes,
        name: page::NOTES,
        owner: Some(NOTES_OWNER),
        title: "Tags and notes",
        icon: "tag",
        intro: "Tags, ratings and comments: how tags are offered, and the places they are kept, so the desktop, a Mac and other applications see them too. Every place is read and merged; each is written as you choose: never, where the file already has notes there, also where a file's volume keeps no attributes (one place is started there, the one its folder already uses first), or for every file edited.",
        when_used: true,
        sections: &["Tags"],
    },
    Builtin {
        page: Page::Reading,
        name: page::READING,
        owner: Some(READING_OWNER),
        title: "Reading",
        icon: "reading",
        intro: "How documents, books, comics, Markdown, text and code read in the preview, and which engine reads each kind.",
        when_used: true,
        sections: &[
            "Pages",
            "Documents",
            "Books",
            "Text",
            "Code",
            "Spreadsheets",
        ],
    },
    Builtin {
        page: Page::Media,
        name: page::MEDIA,
        owner: None,
        title: "Pictures and media",
        icon: "play",
        intro: "How pictures follow one another, how audio and video play, how 3D models are drawn and what maps are drawn on.",
        when_used: true,
        sections: &[
            "Pictures",
            "Playback",
            "Audio",
            "Video",
            "3D models",
            "Maps",
        ],
    },
    Builtin {
        page: Page::Terminal,
        name: page::TERMINAL,
        owner: None,
        title: "Terminal",
        icon: "terminal",
        intro: "The terminal panel, and the shell that runs a command on the selection.",
        when_used: true,
        sections: &["Terminal", "Commands"],
    },
    Builtin {
        page: Page::Network,
        name: page::NETWORK,
        owner: None,
        title: "Network",
        icon: "network",
        intro: "The servers of the sidebar's Network section, added, edited and removed here as from their menus, and where those added by their address are kept; then the tools that open an address.",
        when_used: true,
        sections: &["Servers", "Opening addresses"],
    },
    Builtin {
        page: Page::Privacy,
        name: page::PRIVACY,
        owner: Some(PRIVACY_OWNER),
        title: "Privacy",
        icon: "lock",
        intro: "What leaves this machine. An agent driving skwir, and a plugin from outside skwir, never see a private folder: not its names, not what its files hold. What each plugin may do is said on its line of the Plugins page.",
        when_used: false,
        sections: &[],
    },
    Builtin {
        page: Page::Plugins,
        name: "plugins",
        owner: None,
        title: "Plugins",
        icon: "plugin",
        intro: "Everything beyond the core is a plugin. Turning one off removes its commands, views and settings until it is turned on again.",
        when_used: false,
        sections: &[],
    },
    Builtin {
        page: Page::Keyboard,
        name: page::KEYBOARD,
        owner: Some(skwir_core::keymap::OWNER),
        title: "Keyboard",
        icon: "keyboard",
        intro: "Every action and its keys, grouped by where they work. Click a key to change it, or + to add one: yours replace skwir's on the same keys there.",
        when_used: false,
        sections: &[],
    },
    Builtin {
        page: Page::Changed,
        name: "changed",
        owner: None,
        title: "Changed by you",
        icon: "revert",
        intro: "Every setting that differs from its default and every plugin turned on or off from how skwir comes, each with its way back.",
        when_used: false,
        sections: &[],
    },
];

impl Page {
    /// The page a setting is shown on: the one it names, else its owner's.
    pub fn of_spec(spec: &SettingSpec) -> Page {
        BUILTIN
            .iter()
            .find(|b| !spec.page.is_empty() && b.name == spec.page)
            .map_or_else(|| Page::of_owner(spec.owner()), |b| b.page.clone())
    }

    /// The page of a namespace's settings that name none: one of the
    /// dialog's own (`core` is General), else the plugin's.
    pub fn of_owner(owner: &str) -> Page {
        BUILTIN
            .iter()
            .find(|b| b.owner == Some(owner))
            .map_or_else(|| Page::Plugin(owner.to_owned()), |b| b.page.clone())
    }

    /// A page by its word, or by the namespace it shows (`core` is
    /// General), else the plugin of that id.
    fn parse(name: &str) -> Page {
        BUILTIN
            .iter()
            .find(|b| b.name == name || b.owner == Some(name))
            .map_or_else(|| Page::Plugin(name.to_owned()), |b| b.page.clone())
    }

    /// The dialog's own page this is; `None` for a plugin's.
    pub(crate) fn builtin(&self) -> Option<&'static Builtin> {
        BUILTIN.iter().find(|b| &b.page == self)
    }

    /// The word it is named by: one of the dialog's, else its plugin's id.
    pub(crate) fn word(&self) -> String {
        match self {
            Page::Plugin(id) => id.clone(),
            page => page
                .builtin()
                .map_or_else(String::new, |b| b.name.to_owned()),
        }
    }
}

/// What the page a setting is shown on is called: one of the dialog's
/// own, else its plugin's title, else its namespace.
pub(crate) fn page_title_of(spec: &SettingSpec, plugins: &[PluginInfo]) -> String {
    match Page::of_spec(spec) {
        Page::Plugin(id) => plugins
            .iter()
            .find(|plugin| plugin.id == id)
            .map_or(id, |plugin| plugin.title.clone()),
        page => page
            .builtin()
            .map_or_else(|| spec.owner().to_owned(), |b| b.title.to_owned()),
    }
}

pub enum SettingsEvent {
    Close,
}

pub(crate) struct Row {
    pub spec: SettingSpec,
    /// What the control shows: the live value, or an optimistic one whose
    /// save is queued or in flight.
    pub value: String,
    /// Present for kinds edited as text.
    pub input: Option<Entity<TextInput>>,
    pub anchor: PopoverAnchor,
    _subscriptions: Vec<Subscription>,
}

impl Row {
    pub fn default_text(&self) -> String {
        SettingSpec::format_value(&self.spec.default)
    }

    /// A preference changed from its default. A list of records is data
    /// the user built: no Revert, and Reset all leaves it alone.
    pub fn modified(&self) -> bool {
        self.spec.kind != SettingKind::Records && self.value != self.default_text()
    }

    pub fn page(&self) -> Page {
        Page::of_spec(&self.spec)
    }
}

/// One keyboard stop of the current frame, in traversal order.
pub(crate) struct Stop {
    pub focus: FocusHandle,
    /// Child index inside the scrolling page, to bring it into view.
    pub item: Option<usize>,
    /// The setting a left/right adjustment applies to.
    pub key: Option<String>,
}

/// A long dropdown open (`settings_view::narrows`: more than
/// `settings_view::FILTERED` choices, the icon themes, bat's themes, and
/// every font's): a field above its list keeps the choices holding what is
/// typed, and only the rows on screen are drawn.
pub(crate) struct Narrowed {
    pub(crate) key: String,
    pub(crate) field: Entity<TextInput>,
    /// The choices as the spec had them when opened or last listed:
    /// stored value and label.
    pub(crate) choices: std::rc::Rc<[(String, String)]>,
    /// Which of `choices` the field keeps, in their order.
    pub(crate) kept: Vec<usize>,
    /// The one of `kept` Enter takes, and the one a font's specimen shows:
    /// moved by the keys and by the pointer.
    pub(crate) highlighted: usize,
    pub(crate) scroll: gpui::UniformListScrollHandle,
    _typed: Subscription,
}

impl Narrowed {
    /// The choices of `spec`, as stored values and labels.
    pub(crate) fn choices_of(spec: &SettingSpec) -> std::rc::Rc<[(String, String)]> {
        spec.constraints
            .choices
            .iter()
            .map(|value| (value.clone(), spec.choice_label(value).to_owned()))
            .collect()
    }

    /// Keep the choices whose label holds every word of `query`, case
    /// ignored, the one highlighted staying highlighted while kept.
    pub(crate) fn narrow(&mut self, query: &str) {
        let was = self.highlighted_value().map(str::to_owned);
        self.narrow_keeping(query, was.as_deref());
    }

    /// The choices declared again (the fonts, once listed), narrowed by
    /// `query`: the one highlighted stays, found by its value, never by
    /// its place in the list before.
    pub(crate) fn relist(&mut self, choices: std::rc::Rc<[(String, String)]>, query: &str) {
        let was = self.highlighted_value().map(str::to_owned);
        self.choices = choices;
        self.narrow_keeping(query, was.as_deref());
    }

    /// `narrow`, highlighting the choice of value `was` while kept.
    fn narrow_keeping(&mut self, query: &str, was: Option<&str>) {
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        self.kept = self
            .choices
            .iter()
            .enumerate()
            .filter(|(_, (_, label))| {
                let label = label.to_lowercase();
                words.iter().all(|word| label.contains(word.as_str()))
            })
            .map(|(at, _)| at)
            .collect();
        self.highlighted = was
            .and_then(|was| self.kept.iter().position(|at| self.choices[*at].0 == was))
            .unwrap_or(0);
        self.scroll
            .scroll_to_item(self.highlighted, gpui::ScrollStrategy::Nearest);
    }

    /// The stored value highlighted.
    pub(crate) fn highlighted_value(&self) -> Option<&str> {
        let at = *self.kept.get(self.highlighted)?;
        Some(self.choices[at].0.as_str())
    }
}

// Flags of independent parts of the dialog (a screen reader listening…),
// not the states of one machine.
#[allow(clippy::struct_excessive_bools)]
pub struct SettingsView {
    pub(crate) app: Arc<App>,
    /// The Keyboard page: the keymap as it lists it, and what it edits.
    pub(crate) keys: crate::settings_keys_view::KeysPage,
    pub(crate) page: Page,
    pub(crate) search: Entity<TextInput>,
    pub(crate) rows: Vec<Row>,
    pub(crate) plugins: Vec<PluginInfo>,
    /// `None` until the first probe finishes.
    pub(crate) requirements: Option<Vec<RequirementStatus>>,
    probe: Arc<Mutex<Option<Vec<RequirementStatus>>>>,
    pub(crate) root: FocusHandle,
    /// Whether assistive technology listens this frame: a setting's
    /// title names its control only then.
    pub(crate) a11y: bool,
    pub(crate) nav: FocusHandle,
    handles: HashMap<String, FocusHandle>,
    pub(crate) stops: Vec<Stop>,
    /// Child index of each setting row in the current frame.
    pub(crate) row_items: BTreeMap<String, usize>,
    pub(crate) scroll: gpui::ScrollHandle,
    pub(crate) errors: BTreeMap<String, String>,
    queue: BTreeMap<String, String>,
    inflight: Option<(u64, BTreeMap<String, String>)>,
    pub(crate) saved_flash: bool,
    flash_generation: u64,
    settings_revision: u64,
    plugins_revision: u64,
    /// The records each setting's owner keeps elsewhere (`RecordSource`),
    /// as last listed, by key: the revision listed at and the records'
    /// JSON.
    sourced: HashMap<String, (u64, String)>,
    /// The settings file cannot be written (Home Manager's link): read
    /// when the dialog opens and at each wake, never while drawing.
    pub(crate) read_only: bool,
    /// Plugins whose toggle was requested and has not been observed yet.
    pub(crate) toggling: BTreeMap<String, bool>,
    pub(crate) reset_armed: bool,
    pub(crate) dropdown: Option<String>,
    /// The dropdown open, when it is a long one.
    pub(crate) narrowed: Option<Narrowed>,
    /// Setting to point out after `ui.settings {key}`.
    pub(crate) highlight: Option<String>,
    /// The pages unfolded to show all their settings, by their words
    /// (`UNFOLDED_KEY`); a page without essentials is never folded.
    pub(crate) unfolded: std::collections::BTreeSet<String>,
    /// What the page reached shows whether folded or not, until another is
    /// reached: the settings that differed from their defaults when it was
    /// (and the one a link pointed at), the plugins switched from their
    /// own. Restoring one moves nothing.
    pub(crate) revealed: std::collections::BTreeSet<String>,
    pub(crate) revealed_plugins: std::collections::BTreeSet<String>,
    scroll_to: Option<String>,
    /// The record edited, per records setting (`settings_records`).
    pub(crate) drafts: BTreeMap<String, crate::settings_records::Draft>,
    /// The focus handles of each record's buttons this frame, with the
    /// setting and the record's id: what Alt+Up and Alt+Down move.
    pub(crate) record_stops: Vec<(FocusHandle, String, String)>,
    /// What keyring jobs came back with, for `sync`.
    pub(crate) keyring: Arc<Mutex<Vec<crate::settings_records::KeyringDone>>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<SettingsEvent> for SettingsView {}

impl SettingsView {
    pub fn new(
        app: Arc<App>,
        bound: Arc<Vec<Bound>>,
        target: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| TextInput::new("Search settings", cx));
        let subscriptions = vec![cx.subscribe_in(
            &search,
            window,
            |this, input, event: &InputEvent, window, cx| match event {
                InputEvent::Changed => {
                    this.scroll.set_offset(gpui::Point::default());
                    cx.notify();
                }
                InputEvent::Cancel => {
                    if input.read(cx).text().is_empty() {
                        cx.emit(SettingsEvent::Close);
                    } else {
                        input.update(cx, |input, cx| input.set_text("", cx));
                        cx.notify();
                    }
                }
                InputEvent::Submit => this.step_focus(1, window, cx),
            },
        )];
        let app_unfolded = app
            .settings
            .get::<Vec<String>>(UNFOLDED_KEY)
            .unwrap_or_default()
            .into_iter()
            .collect();
        let mut view = SettingsView {
            plugins: app.plugins.list(),
            settings_revision: app.settings.revision(),
            plugins_revision: app.plugins.revision(),
            sourced: HashMap::new(),
            read_only: app.settings.read_only(),
            keys: crate::settings_keys_view::KeysPage::new(bound, cx),
            app,
            page: Page::General,
            search,
            rows: Vec::new(),
            requirements: None,
            probe: Arc::new(Mutex::new(None)),
            drafts: BTreeMap::new(),
            record_stops: Vec::new(),
            keyring: Arc::default(),
            root: cx.focus_handle(),
            a11y: false,
            nav: cx.focus_handle(),
            handles: HashMap::new(),
            stops: Vec::new(),
            row_items: BTreeMap::new(),
            scroll: gpui::ScrollHandle::new(),
            errors: BTreeMap::new(),
            queue: BTreeMap::new(),
            inflight: None,
            saved_flash: false,
            flash_generation: 0,
            toggling: BTreeMap::new(),
            reset_armed: false,
            dropdown: None,
            narrowed: None,
            highlight: None,
            unfolded: app_unfolded,
            revealed: std::collections::BTreeSet::new(),
            revealed_plugins: std::collections::BTreeSet::new(),
            scroll_to: None,
            _subscriptions: subscriptions,
        };
        view.rebuild(window, cx);
        view.reveal();
        view.read_keys();
        view.probe_requirements();
        list_icon_themes(&view.app);
        list_code_themes(&view.app);
        list_fonts(&view.app, cx.text_system().clone());
        window.focus(&view.nav, cx);
        view.open_target(target, window, cx);
        view
    }

    /// A stable focus handle for a control that has no state of its own.
    pub(crate) fn handle(&mut self, id: &str, cx: &mut Context<Self>) -> FocusHandle {
        if let Some(handle) = self.handles.get(id) {
            return handle.clone();
        }
        let handle = cx.focus_handle();
        self.handles.insert(id.to_owned(), handle.clone());
        handle
    }

    pub(crate) fn query(&self, cx: &gpui::App) -> String {
        self.search.read(cx).text().trim().to_lowercase()
    }

    pub(crate) fn plugin_title(&self, id: &str) -> String {
        self.plugins
            .iter()
            .find(|plugin| plugin.id == id)
            .map_or_else(|| id.to_owned(), |plugin| plugin.title.clone())
    }

    pub(crate) fn page_title(&self, page: &Page) -> String {
        match (page.builtin(), page) {
            (Some(builtin), _) => builtin.title.into(),
            (None, Page::Plugin(id)) => self.plugin_title(id),
            (None, _) => String::new(),
        }
    }

    /// Sidebar entries: the fixed pages, then every plugin that has settings.
    pub(crate) fn pages(&self) -> Vec<Page> {
        let mut pages: Vec<Page> = BUILTIN
            .iter()
            .filter(|b| !b.when_used || self.rows.iter().any(|row| row.page() == b.page))
            .map(|b| b.page.clone())
            .collect();
        let mut owners: Vec<Page> = self
            .rows
            .iter()
            .map(Row::page)
            .filter(|page| matches!(page, Page::Plugin(_)))
            .collect();
        owners.dedup();
        owners.sort_by_key(|page| self.page_title(page).to_lowercase());
        pages.extend(owners);
        pages
    }

    // ------------------------------------------------------------ live state

    fn live(&self, spec: &SettingSpec) -> String {
        if spec.constraints.record_source.is_some() {
            return self
                .sourced
                .get(&spec.key)
                .map_or_else(|| "[]".to_owned(), |(_, records)| records.clone());
        }
        self.app
            .settings
            .text(&spec.key)
            .unwrap_or_else(|| SettingSpec::format_value(&spec.default))
    }

    /// List again the records whose owner keeps them elsewhere and says
    /// they changed since (`RecordSource`), its command answering from
    /// memory: whether any did.
    fn list_sourced(&mut self) -> bool {
        let mut changed = false;
        let sources: Vec<(String, String, u64)> = self
            .rows
            .iter()
            .filter_map(|row| {
                let source = row.spec.constraints.record_source.as_ref()?;
                Some((row.spec.key.clone(), source.list.clone(), source.revision()))
            })
            .collect();
        for (key, list, revision) in sources {
            if self
                .sourced
                .get(&key)
                .is_some_and(|(at, _)| *at == revision)
            {
                continue;
            }
            let records = match self.app.dispatch(&Invocation::new(list.as_str())) {
                Ok(Outcome::Value(value)) => value
                    .get("records")
                    .filter(|records| records.is_array())
                    .map_or_else(|| "[]".to_owned(), ToString::to_string),
                Ok(_) => "[]".to_owned(),
                Err(error) => {
                    tracing::debug!(key, "records not listed: {error:#}");
                    "[]".to_owned()
                }
            };
            if let Some(row) = self.rows.iter_mut().find(|row| row.spec.key == key) {
                row.value.clone_from(&records);
            }
            self.sourced.insert(key, (revision, records));
            changed = true;
        }
        changed
    }

    fn pending(&self, key: &str) -> bool {
        self.queue.contains_key(key)
            || self
                .inflight
                .as_ref()
                .is_some_and(|(_, changes)| changes.contains_key(key))
    }

    /// Reconcile rows with the schema: plugins declare and withdraw settings
    /// as they are toggled, and another surface may have changed a value.
    fn rebuild(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut previous: HashMap<String, Row> = std::mem::take(&mut self.rows)
            .into_iter()
            .map(|row| (row.spec.key.clone(), row))
            .collect();
        let mut specs: Vec<SettingSpec> = self
            .app
            .settings
            .schema()
            .into_iter()
            .filter(SettingSpec::editable)
            .collect();
        page_order(&mut specs);
        for spec in specs {
            let live = self.live(&spec);
            let row = match previous.remove(&spec.key) {
                Some(mut row) if row.spec.kind == spec.kind => {
                    if !self.pending(&spec.key) {
                        row.value.clone_from(&live);
                        if let Some(input) = &row.input
                            && !input.read(cx).is_focused(window)
                        {
                            input.update(cx, |input, cx| input.set_text(live.clone(), cx));
                        }
                    }
                    row.spec = spec;
                    row
                }
                _ => self.new_row(spec, live, window, cx),
            };
            self.rows.push(row);
        }
        self.errors
            .retain(|key, _| self.rows.iter().any(|row| &row.spec.key == key));
    }

    fn new_row(
        &self,
        spec: SettingSpec,
        value: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Row {
        let textual = matches!(
            spec.kind,
            SettingKind::Integer
                | SettingKind::Number
                | SettingKind::Text
                | SettingKind::Path
                | SettingKind::Command
        );
        let mut subscriptions = Vec::new();
        let input = textual.then(|| {
            let placeholder = if spec.kind == SettingKind::Path {
                "Not set"
            } else {
                ""
            };
            let input = cx.new(|cx| TextInput::new(placeholder, cx));
            input.update(cx, |input, cx| {
                input.set_text(value.clone(), cx);
                input.set_label(spec.title.clone());
            });
            let key = spec.key.clone();
            subscriptions.push(cx.subscribe_in(
                &input,
                window,
                move |this, input, event: &InputEvent, _, cx| match event {
                    InputEvent::Changed => {
                        this.errors.remove(&key);
                        this.highlight = None;
                        cx.notify();
                    }
                    InputEvent::Submit => this.commit(&key, cx),
                    InputEvent::Cancel => {
                        let shown = this.row(&key).map(|row| row.value.clone());
                        if shown.as_deref() == Some(input.read(cx).text()) {
                            cx.emit(SettingsEvent::Close);
                        } else if let Some(shown) = shown {
                            input.update(cx, |input, cx| input.set_text(shown, cx));
                            this.errors.remove(&key);
                            cx.notify();
                        }
                    }
                },
            ));
            let key = spec.key.clone();
            subscriptions.push(
                cx.on_blur(&input.focus_handle(cx), window, move |this, _, cx| {
                    this.commit(&key, cx);
                }),
            );
            input
        });
        Row {
            spec,
            value,
            input,
            anchor: PopoverAnchor::default(),
            _subscriptions: subscriptions,
        }
    }

    pub(crate) fn row(&self, key: &str) -> Option<&Row> {
        self.rows.iter().find(|row| row.spec.key == key)
    }

    /// Called by the host whenever the core woke the UI.
    pub fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut changed = false;
        if let Some(id) = self.inflight.as_ref().map(|(id, _)| *id) {
            let (current, state) = self.app.settings.save_status();
            // The core keeps the latest save only, and ids only grow: another
            // id means ours ended and another surface saved since. How ours
            // went is unknown; the live values below say what holds.
            let ended = current != id || state != SaveState::Saving;
            if ended {
                let (_, sent) = self.inflight.take().expect("checked above");
                match state {
                    _ if current != id => {}
                    SaveState::Failed(error) => {
                        for key in sent.keys() {
                            self.errors.insert(key.clone(), error.clone());
                        }
                        self.saved_flash = false;
                    }
                    _ => self.flash(cx),
                }
                // Values of a failed save fall back to the live ones.
                self.settings_revision = u64::MAX;
                changed = true;
            }
        }
        let read_only = self.app.settings.read_only();
        if read_only != self.read_only {
            self.read_only = read_only;
            changed = true;
        }
        let (settings, plugins) = (self.app.settings.revision(), self.app.plugins.revision());
        if plugins != self.plugins_revision {
            self.plugins_revision = plugins;
            self.plugins = self.app.plugins.list();
            let plugins = &self.plugins;
            self.toggling.retain(|id, wanted| {
                plugins
                    .iter()
                    .any(|plugin| plugin.id == id && plugin.enabled != *wanted)
            });
            changed = true;
        }
        if settings != self.settings_revision {
            self.settings_revision = settings;
            self.rebuild(window, cx);
            self.relist_choices(cx);
            changed = true;
        }
        changed |= self.list_sourced();
        if let Some(found) = self.probe.lock().ok().and_then(|mut slot| slot.take()) {
            self.requirements = Some(found);
            changed = true;
        }
        changed |= self.drain_keyring(cx);
        self.flush(cx);
        if changed {
            cx.notify();
        }
    }

    fn flash(&mut self, cx: &mut Context<Self>) {
        self.saved_flash = true;
        self.flash_generation += 1;
        let generation = self.flash_generation;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(1800))
                .await;
            this.update(cx, |this, cx| {
                if this.flash_generation == generation {
                    this.saved_flash = false;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Whether `spec` can be changed: it is under no switch that is off.
    pub(crate) fn available(&self, spec: &SettingSpec) -> bool {
        spec.under.is_empty()
            || self
                .rows
                .iter()
                .find(|row| row.spec.key == spec.under)
                .is_none_or(|row| row.value == "true")
    }

    pub(crate) fn saving(&self) -> bool {
        self.inflight.is_some() || !self.queue.is_empty()
    }

    // ------------------------------------------------------------ changes

    /// Validate and submit one change. Invalid text stays in its field with
    /// the reason below it; nothing is sent.
    pub(crate) fn change(&mut self, key: &str, text: String, cx: &mut Context<Self>) {
        self.highlight = None;
        self.reset_armed = false;
        // A setting under a switch that is off waits for the switch.
        if !self
            .rows
            .iter()
            .find(|row| row.spec.key == key)
            .is_some_and(|row| self.available(&row.spec))
        {
            return;
        }
        let stored = self.app.settings.get::<toml::Value>(key);
        let Some(row) = self.rows.iter_mut().find(|row| row.spec.key == key) else {
            return;
        };
        if let Err(error) = row.spec.parse_over(&text, stored.as_ref()) {
            self.errors.insert(key.to_owned(), format!("{error:#}"));
            cx.notify();
            return;
        }
        self.errors.remove(key);
        if let Some(input) = &row.input
            && input.read(cx).text() != text
        {
            input.update(cx, |input, cx| input.set_text(text.clone(), cx));
        }
        if row.value == text {
            cx.notify();
            return;
        }
        row.value.clone_from(&text);
        self.queue.insert(key.to_owned(), text);
        self.flush(cx);
        cx.notify();
    }

    fn commit(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(text) = self
            .row(key)
            .and_then(|row| row.input.as_ref())
            .map(|input| input.read(cx).text().to_owned())
        else {
            return;
        };
        if self.row(key).is_some_and(|row| row.value != text) {
            self.change(key, text, cx);
        }
    }

    /// Submit the queue unless a save is running; `sync` calls again when it
    /// ends. Another surface (CLI, agent) may hold the save: we wait for it
    /// the same way instead of reporting an error.
    fn flush(&mut self, cx: &mut Context<Self>) {
        if self.inflight.is_some()
            || self.queue.is_empty()
            || self.app.settings.save_status().1 == SaveState::Saving
        {
            return;
        }
        let changes = std::mem::take(&mut self.queue);
        match self.app.commands.dispatch(
            &Invocation::with_args("settings.apply", &serde_json::json!({ "changes": changes })),
            &self.app,
        ) {
            Ok(Outcome::Value(id)) => self.inflight = Some((id.as_u64().unwrap_or(0), changes)),
            Ok(_) => {}
            Err(error) => {
                for key in changes.keys() {
                    self.errors.insert(key.clone(), format!("{error:#}"));
                }
                self.settings_revision = u64::MAX;
            }
        }
        cx.notify();
    }

    /// Left/right on a focused choice or number.
    fn adjust(&mut self, delta: i64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(key) = self
            .stops
            .iter()
            .find(|stop| stop.focus.contains_focused(window, cx))
            .and_then(|stop| stop.key.clone())
        else {
            return;
        };
        // A long list opens rather than step: each step would apply its
        // choice, a font loaded for each one passed.
        let long = self.row(&key).is_some_and(|row| {
            row.spec.kind == SettingKind::Choice && crate::settings_view::narrows(&row.spec)
        });
        if long {
            self.open_choices(&key, window, cx);
        } else {
            self.adjust_key(&key, delta, cx);
        }
    }

    /// The choices a dropdown lists, as declared again (the fonts, once
    /// listed on a worker): the long one open keeps what is typed; a short
    /// one that grew long closes, to open as a long one.
    fn relist_choices(&mut self, cx: &mut Context<Self>) {
        if let Some(narrowed) = &mut self.narrowed
            && let Some(spec) = self.app.settings.spec(&narrowed.key)
        {
            let choices = Narrowed::choices_of(&spec);
            if choices != narrowed.choices {
                let query = narrowed.field.read(cx).text().to_owned();
                narrowed.relist(choices, &query);
            }
        } else if let Some(key) = &self.dropdown
            && self
                .row(key)
                .is_some_and(|row| crate::settings_view::narrows(&row.spec))
        {
            self.dropdown = None;
        }
    }

    pub(crate) fn adjust_key(&mut self, key: &str, delta: i64, cx: &mut Context<Self>) {
        let Some(row) = self.row(key) else { return };
        let text = match row.spec.kind {
            SettingKind::Toggle => (delta > 0).to_string(),
            SettingKind::Choice => {
                let choices = &row.spec.constraints.choices;
                let Some(len) = i64::try_from(choices.len()).ok().filter(|len| *len > 0) else {
                    return;
                };
                let current = choices
                    .iter()
                    .position(|choice| *choice == row.value)
                    .and_then(|index| i64::try_from(index).ok())
                    .unwrap_or(0);
                let next = (current + delta).clamp(0, len - 1);
                choices[usize::try_from(next).unwrap_or(0)].clone()
            }
            SettingKind::Integer => {
                let shown = row
                    .input
                    .as_ref()
                    .and_then(|input| input.read(cx).text().trim().parse::<i64>().ok())
                    .or_else(|| row.value.parse().ok())
                    .or_else(|| row.spec.default.as_integer())
                    .unwrap_or(0);
                let (min, max) = row
                    .spec
                    .constraints
                    .integer_range
                    .unwrap_or((i64::MIN, i64::MAX));
                shown
                    .saturating_add(delta.saturating_mul(row.spec.constraints.step.max(1)))
                    .clamp(min, max)
                    .to_string()
            }
            SettingKind::Number => {
                let shown = row
                    .input
                    .as_ref()
                    .and_then(|input| input.read(cx).text().trim().parse::<f64>().ok())
                    .or_else(|| row.value.parse().ok())
                    .or_else(|| row.spec.default.as_float())
                    .unwrap_or(0.);
                let step = row.spec.constraints.step.max(1) as f64;
                SettingSpec::format_value(&toml::Value::Float(shown + delta as f64 * step))
            }
            _ => return,
        };
        self.change(key, text, cx);
    }

    pub(crate) fn reset(&mut self, key: &str, cx: &mut Context<Self>) {
        if let Some(text) = self.row(key).map(Row::default_text) {
            self.change(key, text, cx);
        }
    }

    pub(crate) fn reset_all(&mut self, cx: &mut Context<Self>) {
        if !self.reset_armed {
            self.reset_armed = true;
            cx.notify();
            return;
        }
        let keys: Vec<String> = self
            .rows
            .iter()
            .filter(|row| row.modified())
            .map(|row| row.spec.key.clone())
            .collect();
        for key in keys {
            self.reset(&key, cx);
        }
        self.reset_armed = false;
    }

    pub(crate) fn browse(&mut self, key: &str, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose".into()),
        });
        let key = key.to_owned();
        cx.spawn(async move |this, cx| {
            let picked = paths.await;
            this.update(cx, |this, cx| match picked {
                Ok(Ok(Some(paths))) => {
                    if let Some(path) = paths.first() {
                        this.change(&key, path.to_string_lossy().into_owned(), cx);
                    }
                }
                Ok(Err(error)) => {
                    this.errors.insert(key.clone(), format!("{error:#}"));
                    cx.notify();
                }
                _ => {}
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn set_plugin(&mut self, id: &str, enabled: bool, cx: &mut Context<Self>) {
        match self.app.commands.dispatch(
            &Invocation::with_args(
                "plugins.set_enabled",
                &serde_json::json!({ "id": id, "enabled": enabled }),
            ),
            &self.app,
        ) {
            Ok(_) => {
                self.toggling.insert(id.to_owned(), enabled);
            }
            Err(error) => self
                .app
                .notify(skwir_core::app::NotifyLevel::Error, format!("{error:#}")),
        }
        cx.notify();
    }

    pub(crate) fn probe_requirements(&mut self) {
        let slot = Arc::clone(&self.probe);
        let weak = self.app.weak();
        self.app.scheduler.spawn(
            skwir_core::scheduler::Priority::Normal,
            "settings-requirements",
            self.app.lifetime().child(),
            move || {
                let Some(app) = weak.upgrade() else { return };
                let found = app.plugins.probe_requirements();
                if let Ok(mut slot) = slot.lock() {
                    *slot = Some(found);
                }
                app.wake();
            },
        );
    }

    /// Open the settings file in the user's editor. The file exists only
    /// once something differs from the defaults.
    pub(crate) fn open_file(&self) {
        let Some(path) = self.app.settings.path().map(std::path::Path::to_path_buf) else {
            return;
        };
        let weak = self.app.weak();
        self.app.scheduler.spawn(
            skwir_core::scheduler::Priority::Normal,
            "settings-open-file",
            self.app.lifetime().child(),
            move || {
                use skwir_core::app::NotifyLevel;
                let Some(app) = weak.upgrade() else { return };
                if !path.is_file() {
                    app.notify(
                        NotifyLevel::Info,
                        "There is no settings file yet: every setting has its default value",
                    );
                } else if let Err(error) = skwir_platform::open::open_path(&path) {
                    app.notify(NotifyLevel::Error, format!("{error:#}"));
                }
            },
        );
    }

    // ------------------------------------------------------------ navigation

    pub(crate) fn go(&mut self, page: Page, cx: &mut Context<Self>) {
        self.search.update(cx, |input, cx| input.set_text("", cx));
        self.page = page;
        self.dropdown = None;
        self.reset_armed = false;
        self.keys.editor = None;
        self.keys.new = None;
        self.scroll.set_offset(gpui::Point::default());
        self.reveal();
        cx.notify();
    }

    /// What a page reached shows whether folded or not (`revealed`).
    fn reveal(&mut self) {
        self.revealed = self
            .rows
            .iter()
            .filter(|row| row.modified())
            .map(|row| row.spec.key.clone())
            .collect();
        self.revealed_plugins = self
            .plugins
            .iter()
            .filter(|plugin| plugin.enabled != plugin.enabled_by_default)
            .map(|plugin| plugin.id.to_owned())
            .collect();
    }

    /// Whether a page shows only its essentials: it has some, and was not
    /// unfolded.
    pub(crate) fn folded(&self, page: &Page) -> bool {
        !self.unfolded.contains(&page.word())
            && self
                .rows
                .iter()
                .any(|row| row.spec.essential && row.page() == *page)
    }

    /// Fold or unfold a page, as the window's state remembers.
    pub(crate) fn fold(&mut self, page: &Page, folded: bool, cx: &mut Context<Self>) {
        let word = page.word();
        if folded {
            self.unfolded.remove(&word);
        } else {
            self.unfolded.insert(word);
        }
        self.app.settings.set(
            UNFOLDED_KEY,
            toml::Value::Array(
                self.unfolded
                    .iter()
                    .cloned()
                    .map(toml::Value::String)
                    .collect(),
            ),
        );
        let weak = self.app.weak();
        self.app.scheduler.spawn(
            skwir_core::scheduler::Priority::Background,
            "settings-fold-save",
            self.app.lifetime().child(),
            move || {
                if let Some(app) = weak.upgrade()
                    && let Err(error) = app.settings.save()
                {
                    tracing::warn!("cannot save the window's state: {error:#}");
                }
            },
        );
        cx.notify();
    }

    /// How many settings differ from their defaults and plugins from how
    /// skwir comes, as Changed by you lists them.
    pub(crate) fn changed_count(&self) -> usize {
        self.rows.iter().filter(|row| row.modified()).count()
            + self
                .plugins
                .iter()
                .filter(|plugin| plugin.enabled != plugin.enabled_by_default)
                .count()
    }

    /// `{ "key": "plugin.option" }` or `{ "page": "plugins" }`; a plugin's
    /// id as the page goes where its settings are.
    pub fn open_target(
        &mut self,
        target: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = |name: &str| target.get(name).and_then(serde_json::Value::as_str);
        if let Some(key) = text("key") {
            let page = self.row(key).map_or_else(
                || Page::of_owner(skwir_core::settings::owner_of(key)),
                Row::page,
            );
            self.go(page, cx);
            if self.row(key).is_some() {
                self.highlight = Some(key.to_owned());
                self.scroll_to = Some(key.to_owned());
                // Shown whether its page is folded or not.
                self.revealed.insert(key.to_owned());
            }
        } else if let Some(name) = text("page") {
            match Page::parse(name) {
                Page::Plugin(id)
                    if !self
                        .rows
                        .iter()
                        .any(|row| row.page() == Page::Plugin(id.clone())) =>
                {
                    self.go_to_plugin(&id, cx);
                }
                page => self.go(page, cx),
            }
        }
        window.focus(&self.nav, cx);
    }

    /// The page a plugin's settings are on, at the first of them: its own
    /// page, or the one of their subject (ADR-072).
    pub(crate) fn go_to_plugin(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some((page, key)) = self
            .rows
            .iter()
            .find(|row| row.spec.owner() == id)
            .map(|row| (row.page(), row.spec.key.clone()))
        else {
            self.go(Page::Plugin(id.to_owned()), cx);
            return;
        };
        self.go(page, cx);
        // All of them, whether their page is folded or not.
        let owned: Vec<String> = self
            .rows
            .iter()
            .filter(|row| row.spec.owner() == id)
            .map(|row| row.spec.key.clone())
            .collect();
        self.revealed.extend(owned);
        self.scroll_to = Some(key);
    }

    /// The row to scroll to once it has been laid out, consumed by render.
    pub(crate) fn take_scroll_target(&mut self) -> Option<String> {
        self.scroll_to.take()
    }

    fn step_page(&mut self, delta: isize, cx: &mut Context<Self>) {
        let pages = self.pages();
        let current = pages
            .iter()
            .position(|page| *page == self.page)
            .unwrap_or(0);
        let next = current
            .saturating_add_signed(delta)
            .min(pages.len().saturating_sub(1));
        self.go(pages[next].clone(), cx);
    }

    /// Open the long dropdown of `key`, its field taking the keyboard and
    /// the value chosen highlighted.
    pub(crate) fn open_choices(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(spec) = self.app.settings.spec(key) else {
            return;
        };
        let value = self
            .row(key)
            .map(|row| row.value.clone())
            .unwrap_or_default();
        let field = cx.new(|cx| TextInput::new("Type to narrow", cx));
        let typed = cx.subscribe_in(
            &field,
            window,
            |this, field, event: &InputEvent, window, cx| match event {
                InputEvent::Changed => {
                    let query = field.read(cx).text().to_owned();
                    if let Some(narrowed) = &mut this.narrowed {
                        narrowed.narrow(&query);
                    }
                    cx.notify();
                }
                InputEvent::Submit => this.take_choice(window, cx),
                InputEvent::Cancel => this.close_choices(window, cx),
            },
        );
        let mut narrowed = Narrowed {
            key: key.to_owned(),
            field: field.clone(),
            choices: Narrowed::choices_of(&spec),
            kept: Vec::new(),
            highlighted: 0,
            scroll: gpui::UniformListScrollHandle::new(),
            _typed: typed,
        };
        narrowed.narrow("");
        if let Some(at) = narrowed.choices.iter().position(|(v, _)| *v == value) {
            narrowed.highlighted = at;
            narrowed
                .scroll
                .scroll_to_item(at, gpui::ScrollStrategy::Center);
        }
        self.narrowed = Some(narrowed);
        self.dropdown = Some(key.to_owned());
        field.update(cx, |field, cx| field.focus(window, cx));
        cx.notify();
    }

    /// Close the long dropdown, the keyboard back on its control.
    pub(crate) fn close_choices(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dropdown = None;
        if let Some(narrowed) = self.narrowed.take() {
            let control = self.handle(&format!("control:{}", narrowed.key), cx);
            window.focus(&control, cx);
        }
        cx.notify();
    }

    /// Take the choice highlighted.
    pub(crate) fn take_choice(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(narrowed) = &self.narrowed else {
            return;
        };
        let key = narrowed.key.clone();
        let value = narrowed.highlighted_value().map(str::to_owned);
        self.close_choices(window, cx);
        if let Some(value) = value {
            self.change(&key, value, cx);
        }
    }

    fn move_choices(&mut self, mv: crate::modal::ListMove, cx: &mut Context<Self>) {
        let Some(narrowed) = &mut self.narrowed else {
            return;
        };
        if narrowed.kept.is_empty() {
            return;
        }
        narrowed.highlighted = mv.from(narrowed.highlighted, narrowed.kept.len());
        narrowed
            .scroll
            .scroll_to_item(narrowed.highlighted, gpui::ScrollStrategy::Nearest);
        cx.notify();
    }

    fn step_focus(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        self.dropdown = None;
        self.narrowed = None;
        if self.stops.is_empty() {
            return;
        }
        let len = self.stops.len();
        let next = match self
            .stops
            .iter()
            .position(|stop| stop.focus.contains_focused(window, cx))
        {
            Some(current) => (current + len).saturating_add_signed(delta) % len,
            None if delta > 0 => 0,
            None => len - 1,
        };
        let stop = &self.stops[next];
        if let Some(item) = stop.item {
            self.scroll.scroll_to_item(item);
        }
        window.focus(&stop.focus, cx);
        cx.notify();
    }

    /// `ui.settings_*` commands, from key bindings and other surfaces.
    pub fn command(
        &mut self,
        name: &str,
        args: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let delta = args
            .get("delta")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(1);
        match name {
            "settings_close" => {
                if self.narrowed.is_some() {
                    self.close_choices(window, cx);
                } else if self.dropdown.take().is_some() {
                    cx.notify();
                } else if self.keys.editor.is_some() {
                    self.close_key_editor(window, cx);
                } else if self.keys.new.take().is_some() {
                    window.focus(&self.keys.focus, cx);
                    cx.notify();
                } else {
                    cx.emit(SettingsEvent::Close);
                }
            }
            "settings_list_move" => self.move_choices(crate::modal::ListMove::of(args), cx),
            "settings_next" => self.step_focus(1, window, cx),
            "settings_prev" => self.step_focus(-1, window, cx),
            "settings_search" => {
                self.search.update(cx, |input, cx| {
                    input.select_all_now(cx);
                    input.focus(window, cx);
                });
            }
            "settings_page" => self.step_page(isize::try_from(delta).unwrap_or(1), cx),
            "settings_enter" => {
                // From the category list into the page: the stop after it.
                window.focus(&self.nav, cx);
                self.step_focus(1, window, cx);
            }
            "settings_adjust" => self.adjust(delta, window, cx),
            "settings_move" => self.move_focused_record(delta, window, cx),
            "settings_key" => {
                let key = args
                    .get("key")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default();
                self.keys_key(key, window, cx);
            }
            _ => {}
        }
    }
}

impl Focusable for SettingsView {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.root.clone()
    }
}

// ------------------------------------------------------------ hosting

impl crate::shell::Shell {
    /// Open the dialog, or point the open one at `{ key }` / `{ page }`.
    pub fn open_settings(
        &mut self,
        target: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(settings) = &self.settings {
            settings.update(cx, |settings, cx| settings.open_target(target, window, cx));
            return;
        }
        self.palette = None;
        self.menu = None;
        if let Some(popup) = self.toolbar_menu.take() {
            popup.restore(window, cx);
        }
        let (app, bound) = (Arc::clone(&self.app), self.keys.bound());
        // What had the keyboard gets it back when the dialog closes, as a
        // popup's does: the reader, the address, the pane.
        let mut back = Some(crate::modal::FocusReturn::capture(window, cx));
        let settings = cx.new(|cx| SettingsView::new(app, bound, target, window, cx));
        self.settings_subscription = Some(cx.subscribe_in(
            &settings,
            window,
            move |this, _, event: &SettingsEvent, window, cx| match event {
                SettingsEvent::Close => {
                    this.settings = None;
                    this.settings_subscription = None;
                    let fallback = this.ui().focus.clone();
                    back.take()
                        .unwrap_or_default()
                        .restore(window, &fallback, cx);
                    cx.notify();
                }
            },
        ));
        self.settings = Some(settings);
    }

    pub fn settings_command(
        &mut self,
        name: &str,
        args: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(settings) = self.settings.clone() {
            settings.update(cx, |settings, cx| settings.command(name, args, window, cx));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        App, BUILTIN, CODE_APPEARANCE_KEY, CODE_DARK_KEY, CODE_FONT_KEY, CODE_LIGHT_KEY, FONT_KEY,
        ICON_THEME_AUTO, Page, READING_FONT_KEY, SINGLE_TAB_KEY, SettingSpec, THEME_KEY,
        code_theme_spec, declare, font_spec, icon_theme_spec, page_order,
    };
    use skwir_platform::icons::Theme;

    #[test]
    fn a_font_is_automatic_or_a_family_the_text_system_draws() {
        let installed = crate::fonts::Installed {
            families: vec!["DejaVu Sans".into(), "Hack".into(), "Noto Serif".into()],
            monospaced: Some(["Hack".to_owned()].into()),
        };
        let listed = font_spec(FONT_KEY, "auto", Some(&installed));
        assert_eq!(
            listed.constraints.choices,
            ["auto", "DejaVu Sans", "Hack", "Noto Serif"]
        );
        assert_eq!(listed.constraints.choice_labels[0], "Automatic");
        assert_eq!(listed.default.as_str(), Some("auto"));
        assert!(!listed.constraints.more_choices.is_empty());
        // Code is offered the monospaced ones, reading every one.
        let code = font_spec(CODE_FONT_KEY, "auto", Some(&installed));
        assert_eq!(code.constraints.choices, ["auto", "Hack"]);
        // However few, a font's list is the one that narrows and writes
        // the family highlighted in itself; a short choice of another
        // kind stays a plain dropdown.
        assert!(crate::settings_view::narrows(&code));
        assert!(!crate::settings_view::narrows(&SettingSpec::choice(
            "a.look",
            "Look",
            "Looks.",
            "one",
            &[("one", "One"), ("two", "Two")],
        )));
        let reading = font_spec(READING_FONT_KEY, "Noto Serif", Some(&installed));
        assert_eq!(reading.constraints.choices.len(), 4);
        assert!(reading.essential, "people are divided on it");
        // Chosen, not listed yet: a choice of its own name.
        let early = font_spec(CODE_FONT_KEY, "Iosevka", None);
        assert_eq!(early.constraints.choices, ["auto", "Iosevka"]);
        assert_eq!(early.constraints.choice_labels, ["Automatic", "Iosevka"]);
        // A proportional family chosen for code is kept, not refused.
        let kept = font_spec(CODE_FONT_KEY, "Noto Serif", Some(&installed));
        assert_eq!(
            kept.constraints.choice_labels.last().map(String::as_str),
            Some("Noto Serif")
        );
        // Chosen and no longer installed: said so.
        let gone = font_spec(CODE_FONT_KEY, "Iosevka", Some(&installed));
        assert_eq!(
            gone.constraints.choice_labels.last().map(String::as_str),
            Some("Iosevka (not installed)")
        );
    }

    #[test]
    fn the_icon_theme_is_automatic_or_one_installed_by_its_folder() {
        let themes = [
            Theme {
                folder: "Adwaita".into(),
                name: "Adwaita".into(),
            },
            Theme {
                folder: "breeze".into(),
                name: "Breeze".into(),
            },
        ];
        let listed = icon_theme_spec(ICON_THEME_AUTO, Some(&themes));
        assert_eq!(listed.constraints.choices, ["auto", "Adwaita", "breeze"]);
        assert_eq!(
            listed.constraints.choice_labels,
            ["Automatic", "Adwaita", "Breeze"]
        );
        assert_eq!(listed.default.as_str(), Some("auto"));
        // Chosen and listed: no more than its name.
        let chosen = icon_theme_spec("breeze", Some(&themes));
        assert_eq!(chosen.constraints.choices.len(), 3);
        // Chosen, not listed yet: a choice under its folder's name.
        let early = icon_theme_spec("Papirus", None);
        assert_eq!(early.constraints.choices, ["auto", "Papirus"]);
        assert_eq!(early.constraints.choice_labels, ["Automatic", "Papirus"]);
        // Chosen and no longer installed: said so.
        let gone = icon_theme_spec("Papirus", Some(&themes));
        assert_eq!(
            gone.constraints.choices.last().map(String::as_str),
            Some("Papirus")
        );
        assert_eq!(
            gone.constraints.choice_labels.last().map(String::as_str),
            Some("Papirus (not installed)")
        );
    }

    /// A page shows its settings by subject (ADR-072), its sections in the
    /// order it lists them, each section's settings as they were declared:
    /// Theme first, the list's columns in the order they stand. Declared
    /// again, a setting keeps its place.
    #[test]
    fn a_page_shows_its_settings_by_subject_as_declared() {
        let tmp = tempfile::tempdir().unwrap();
        let app = App::new(skwir_core::app::AppConfig::ephemeral(tmp.path())).unwrap();
        declare(&app);
        let pages = || {
            let mut specs: Vec<SettingSpec> = app
                .settings
                .schema()
                .into_iter()
                .filter(SettingSpec::editable)
                .collect();
            page_order(&mut specs);
            specs
        };
        let page = |specs: &[SettingSpec], page: &Page| -> Vec<(String, String)> {
            specs
                .iter()
                .filter(|spec| Page::of_spec(spec) == *page)
                .map(|spec| (spec.section.clone(), spec.key.clone()))
                .collect()
        };
        // Each section once, in the page's order.
        let sections = |specs: &[SettingSpec], at: &Page| -> Vec<String> {
            let mut seen: Vec<String> = Vec::new();
            for (section, _) in page(specs, at) {
                if seen.last() != Some(&section) {
                    seen.push(section);
                }
            }
            seen
        };
        let specs = pages();
        // A page's settings stand together, in the sidebar's order.
        let order: Vec<Page> = specs.iter().map(Page::of_spec).collect();
        let mut seen: Vec<Page> = order.clone();
        seen.dedup();
        let mut once = seen.clone();
        once.sort_by_key(|page| BUILTIN.iter().position(|b| b.page == *page));
        once.dedup();
        assert_eq!(seen, once);
        assert_eq!(page(&specs, &Page::Appearance)[0].1, THEME_KEY);
        assert_eq!(sections(&specs, &Page::Appearance), [""]);
        assert_eq!(
            sections(&specs, &Page::General),
            ["Startup", "Tabs", "Split", "Address", "Drag and drop"]
        );
        // The core's and the shell's settings of a subject meet.
        assert!(
            page(&specs, &Page::General)
                .iter()
                .any(|(section, key)| section == "Tabs" && key == SINGLE_TAB_KEY)
        );
        assert_eq!(
            sections(&specs, &Page::Views),
            ["Folders", "Grid", "List columns"]
        );
        let columns: Vec<String> = page(&specs, &Page::Views)
            .into_iter()
            .filter(|(section, _)| section == "List columns")
            .map(|(_, key)| key)
            .collect();
        let standing: Vec<&str> = crate::views::list::COLUMNS
            .iter()
            .map(|column| column.setting)
            .collect();
        assert_eq!(columns, standing);
        assert_eq!(sections(&specs, &Page::Reading), ["Pages", "Text", "Code"]);
        assert_eq!(
            page(&specs, &Page::Reading)
                .into_iter()
                .filter(|(section, _)| section == "Code")
                .map(|(_, key)| key)
                .collect::<Vec<_>>(),
            [
                CODE_APPEARANCE_KEY,
                CODE_LIGHT_KEY,
                CODE_DARK_KEY,
                CODE_FONT_KEY
            ]
        );
        // The fonts stand by their subjects (ADR-073): the interface's on
        // Appearance, the reading font first of what text reads in.
        assert!(
            page(&specs, &Page::Appearance)
                .iter()
                .any(|(_, key)| key == FONT_KEY)
        );
        assert_eq!(
            page(&specs, &Page::Reading)
                .into_iter()
                .find(|(section, _)| section == "Text")
                .map(|(_, key)| key)
                .as_deref(),
            Some(READING_FONT_KEY)
        );
        assert_eq!(sections(&specs, &Page::Media), ["Pictures", "3D models"]);
        assert_eq!(sections(&specs, &Page::Notes), ["Tags"]);
        // The code themes are declared again once bat's are read.
        app.settings.declare(code_theme_spec(
            false,
            crate::code_preview::DEFAULT,
            Some(&[][..]),
        ));
        assert_eq!(page(&pages(), &Page::Reading), page(&specs, &Page::Reading));
    }

    /// Sections made of the same settings are one table, a line each;
    /// another section between them, a section of records or one setting
    /// under a switch keeps them apart.
    #[test]
    fn sections_alike_are_a_table() {
        use super::{Block, blocks};
        use skwir_core::settings::SettingKind;
        let spec = |key: &str, title: &str, section: &str| {
            SettingSpec::toggle(key, title, "Does.", true).section(section)
        };
        let specs = [
            spec("a.x", "Alone", ""),
            spec("a.one_read", "Read", "One"),
            spec("a.one_write", "Write", "One"),
            spec("a.two_read", "Read", "Two"),
            spec("a.two_write", "Write", "Two"),
            spec("a.three", "Other", "Three"),
            spec("a.four_read", "Read", "Four"),
            spec("a.four_write", "Write", "Four"),
        ];
        let refs: Vec<&SettingSpec> = specs.iter().collect();
        assert_eq!(
            blocks(&refs),
            [
                Block::Section {
                    title: String::new(),
                    rows: vec![0]
                },
                Block::Table {
                    columns: vec![
                        ("Read".into(), SettingKind::Toggle),
                        ("Write".into(), SettingKind::Toggle)
                    ],
                    lines: vec![("One".into(), vec![1, 2]), ("Two".into(), vec![3, 4])],
                },
                Block::Section {
                    title: "Three".into(),
                    rows: vec![5]
                },
                Block::Section {
                    title: "Four".into(),
                    rows: vec![6, 7]
                },
            ]
        );
        // A setting under a switch is no column.
        let under = [
            spec("a.one", "On", "One"),
            spec("a.one_more", "More", "One").under("a.one"),
            spec("a.two", "On", "Two"),
            spec("a.two_more", "More", "Two").under("a.two"),
        ];
        let refs: Vec<&SettingSpec> = under.iter().collect();
        assert!(
            blocks(&refs)
                .iter()
                .all(|block| matches!(block, Block::Section { .. }))
        );
    }

    #[test]
    fn a_row_says_its_first_sentence() {
        use super::first_sentence;
        assert_eq!(first_sentence("One. Two."), ("One.", true));
        assert_eq!(first_sentence("Alone."), ("Alone.", false));
        // Not every full stop ends one: a name, a file's extension.
        assert_eq!(
            first_sentence("A Mac's ._ files, .ts folders. Then more."),
            ("A Mac's ._ files, .ts folders.", true)
        );
        assert_eq!(
            first_sentence("Friendly dates read as 12 Mar. ISO shows 2026-03-12."),
            ("Friendly dates read as 12 Mar.", true)
        );
        assert_eq!(
            first_sentence("Pandoc reads them. skwir reads Word."),
            ("Pandoc reads them.", true)
        );
    }

    /// Every page a setting may name is one of the dialog's.
    #[test]
    fn every_page_a_setting_names_is_drawn() {
        use skwir_core::settings::page;
        for name in [
            page::GENERAL,
            page::APPEARANCE,
            page::VIEWS,
            page::NOTES,
            page::READING,
            page::MEDIA,
            page::TERMINAL,
            page::NETWORK,
            page::PRIVACY,
            page::KEYBOARD,
        ] {
            let spec = SettingSpec::toggle("plugin.option", "Option", "Does.", true).page(name);
            assert!(
                Page::of_spec(&spec)
                    .builtin()
                    .is_some_and(|b| b.name == name),
                "{name}"
            );
        }
        // One that names none is on its owner's page.
        let spec = SettingSpec::toggle("plugin.option", "Option", "Does.", true);
        assert_eq!(Page::of_spec(&spec), Page::Plugin("plugin".into()));
        let spec = SettingSpec::toggle("core.option", "Option", "Does.", true);
        assert_eq!(Page::of_spec(&spec), Page::General);
    }

    #[test]
    fn a_page_is_named_by_its_word_or_its_namespace() {
        assert_eq!(Page::parse("general"), Page::General);
        assert_eq!(Page::parse("core"), Page::General);
        assert_eq!(Page::parse("ui"), Page::Appearance);
        assert_eq!(Page::parse("plugins"), Page::Plugins);
        assert_eq!(Page::parse("tags"), Page::Plugin("tags".into()));
        assert_eq!(Page::parse("views"), Page::Views);
        assert_eq!(Page::parse("terminal"), Page::Terminal);
        assert_eq!(Page::parse("network"), Page::Network);
        assert_eq!(Page::of_owner(skwir_core::keymap::OWNER), Page::Keyboard);
        assert!(Page::Plugin("tags".into()).builtin().is_none());
    }
}
