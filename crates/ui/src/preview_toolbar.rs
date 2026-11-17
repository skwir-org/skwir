//! Common preview controls; document-only tools remain in the reader.
use crate::sizes::TextSizes;
use crate::{
    preview::PreviewContent,
    preview_bar::{Bar, Hex, Item, Looks, Slot, Whole, label},
    shell::Shell,
    theme::Theme,
    toolbar::{self, Button},
};
use gpui::{AnyElement, Context, Div, div, prelude::*};
use skwir_core::preview::PreviewInfo;
use skwir_core::words;
use skwir_core::words::{Decimal, Words};

pub fn zoom_label(shell: &Shell) -> Words {
    shell.preview.zoom.map_or_else(
        || {
            if shell.preview.fit_width {
                words!(
                    preview_zoom_width = "Width",
                    note = "On a preview's zoom button, where a percentage would be: the picture or page fits the width."
                )
            } else {
                words!(
                    preview_zoom_fit = "Fit",
                    note = "On a preview's zoom button, where a percentage would be: the picture or page fits the room there is."
                )
            }
        },
        |zoom| zoom_percent(f64::from(zoom)),
    )
}

/// A zoom, as a percentage of the picture's or page's own size.
fn zoom_percent(zoom: f64) -> Words {
    words!(
        preview_zoom_percent = "{ $percent }%",
        percent = Decimal::of(zoom * 100., 0),
        note = "On a preview's zoom button and among its choices: the zoom, a percentage of the picture's or page's own size (150%)."
    )
}

/// The zoom button's own popup: a picker of values, like a combo box. A
/// film has no Fit width (`preview::ShownAs::Film`).
fn zoom_menu(shell: &Shell, cx: &mut Context<Shell>) -> Div {
    let mut menu = div().flex().flex_col().gap_1();
    let fits = [
        ("menu-fit", "fit", label::fit(), "reset"),
        ("menu-width", "width", label::fit_width(), "width"),
    ];
    let film = shell.preview.shows_film();
    for (id, icon, label, value) in fits.into_iter().filter(|fit| !(film && fit.3 == "width")) {
        menu = menu.child(
            Button::new(id, icon, label.clone(), "ui.preview_zoom")
                .label(label)
                .args(serde_json::json!({"dir":value}))
                .active(if value == "width" {
                    shell.preview.fit_width
                } else {
                    shell.preview.zoom.is_none() && !shell.preview.fit_width
                })
                .menu()
                .render(shell, cx),
        );
    }
    let mut presets = div().flex().flex_wrap().gap_1();
    for (id, zoom, value) in [
        ("zoom50", 0.5, "0.5"),
        ("zoom75", 0.75, "0.75"),
        ("zoom100", 1., "1"),
        ("zoom150", 1.5, "1.5"),
        ("zoom200", 2., "2"),
        ("zoom400", 4., "4"),
    ] {
        let label = zoom_percent(zoom);
        presets = presets.child(
            Button::new(id, "", label.clone(), "ui.preview_zoom")
                .label(label)
                .args(serde_json::json!({"dir":value}))
                .render(shell, cx),
        );
    }
    menu.child(presets)
}

/// Zoom, for whatever is looked at rather than read: a picture, a film or
/// a page. The same five tools in the same order in all, but a film's four:
/// it has no Fit width, its Fit filling the panel (`preview::ShownAs::Film`).
/// A book laid out into pages by its engine is read: its steps size its
/// text (`Shell::preview_zoom`), and say so.
pub fn zoom_tools(bar: &mut Bar, shell: &Shell) {
    let fit = shell.preview.zoom.is_none() && !shell.preview.fit_width;
    let step = |id, icon, title, dir| {
        Button::new(id, icon, title, "ui.preview_zoom").args(serde_json::json!({"dir":dir}))
    };
    let (smaller, larger) = if shell.reader_reflows() {
        (label::smaller_text(), label::larger_text())
    } else {
        (label::zoom_out(), label::zoom_in())
    };
    bar.push(Item::button(
        Slot::View,
        1,
        step("preview-zoom-out", "minus", smaller, "out"),
    ));
    bar.push(
        Item::button(
            Slot::View,
            2,
            Button::new(
                "preview-zoom-menu",
                "chevron",
                label::zoom(),
                "ui.preview_menu",
            )
            .args(serde_json::json!({"kind":"zoom"}))
            .label(zoom_label(shell)),
        )
        .opens("zoom"),
    );
    bar.push(Item::button(
        Slot::View,
        1,
        step("preview-zoom-in", "plus", larger, "in"),
    ));
    bar.push(Item::button(
        Slot::View,
        1,
        step("preview-fit", "fit", label::fit(), "reset").active(fit),
    ));
    if !shell.preview.shows_film() {
        bar.push(Item::button(
            Slot::View,
            0,
            step("preview-width", "width", label::fit_width(), "width")
                .active(shell.preview.fit_width),
        ));
    }
    bar.popups.push((
        "zoom",
        std::rc::Rc::new(|shell, theme, cx| {
            let popup = shell.preview.menu.as_ref()?;
            Some(
                toolbar::menu_panel(popup, "ui.preview_menu", theme, cx)
                    .child(zoom_menu(shell, cx))
                    .into_any_element(),
            )
        }),
    ));
}

/// Whether the bytes of what is shown can be viewed: the hexadecimal
/// inspection provider is registered, and what is shown is a file here
/// whose bytes are read (`PreviewState::shows_bytes`: not a folder, a
/// special file nor a link that leads nowhere), not a member asked for that
/// was never written out (too large, refused). The registry lock is taken
/// only when its revision changed since the last read, never once per
/// variant per frame.
pub(crate) fn hex_available(shell: &Shell) -> bool {
    if shell.preview.descend.is_some() || !shell.preview.shows_bytes() {
        return false;
    }
    let revision = shell.app.previews.revision();
    let cache = &shell.preview.hex_provider;
    if let Some((seen, hex)) = cache.get()
        && seen == revision
    {
        return hex;
    }
    let hex = shell.app.previews.bytes_view().is_some();
    cache.set(Some((revision, hex)));
    hex
}

/// Whether the panel shows nothing of a file it could read, and nothing
/// but its bytes can: its preview failed, or it has none. Not a file too
/// large for a quick preview, which Full preview shows, nor a link that
/// leads nowhere nor a special file, which are never read
/// (`PreviewKey::content`).
fn shows_nothing(shell: &Shell) -> bool {
    matches!(
        shell.preview.content,
        Some(PreviewContent::Error(_) | PreviewContent::Generic)
    ) && shell.preview.shown_content().is_some()
}

/// What the bar says about content with no bar of its own. An inspection
/// has its own (`inspector::toolbar`), so it never comes here.
fn caption(shell: &Shell) -> String {
    match &shell.preview.content {
        Some(PreviewContent::Image {
            width,
            height,
            detail: Some(PreviewInfo::Video { .. }),
            ..
        }) => words!(
            preview_video_still = "Video still · { $width }\u{a0}×\u{a0}{ $height }",
            width = *width,
            height = *height,
            note = "A preview's caption while it shows a picture of a film: its size in pixels, width by height (1920 × 1080)."
        )
        .say(),
        Some(PreviewContent::Image { width, height, .. }) => {
            crate::format::dimensions(u64::from(*width), u64::from(*height))
        }
        Some(PreviewContent::Error(_)) => words!(
            preview_unavailable = "Preview unavailable",
            note = "A preview's caption where reading the file failed."
        )
        .say(),
        Some(PreviewContent::TooLarge(_)) => words!(
            preview_large_file = "Large file",
            note = "A preview's caption for a file too large for a quick preview."
        )
        .say(),
        Some(PreviewContent::Generic) => words!(
            preview_none = "No preview",
            note = "A preview's caption for a file nothing shows."
        )
        .say(),
        // Only once the wait has lasted: a caption that flashes by is noise.
        None if shell.preview_busy() => words!(
            preview_loading = "Loading…",
            note = "A preview's caption while the file is read."
        )
        .say(),
        // A folder has no bar (`preview::controls`).
        None | Some(PreviewContent::Inspection(_) | PreviewContent::Dir { .. }) => String::new(),
    }
}

pub fn render(shell: &Shell, theme: &Theme, cx: &mut Context<Shell>) -> AnyElement {
    let image = matches!(shell.preview.content, Some(PreviewContent::Image { .. }));
    let mut bar = Bar::new("preview-tools");
    crate::preview_bar::close_tool(&mut bar, shell, theme);
    if let Some(PreviewContent::Image {
        detail: Some(PreviewInfo::Video {
            seconds, duration, ..
        }),
        ..
    }) = &shell.preview.content
    {
        let current = shell.preview.seconds.unwrap_or(*seconds);
        let time = match duration {
            None => words!(
                preview_video_at = "{ $seconds }\u{a0}s",
                seconds = Decimal::of(current, 1),
                note = "Where a film preview stands, in seconds to a tenth (1.5 s)."
            ),
            Some(total) => words!(
                preview_video_at_of = "{ $seconds }\u{a0}s / { $total }\u{a0}s",
                seconds = Decimal::of(current, 1),
                total = Decimal::of(*total, 1),
                note = "Where a film preview stands and how long it is, in seconds to a tenth (1.5 s / 3.0 s)."
            ),
        }
        .say();
        bar.push(Item::button(
            Slot::Navigate,
            2,
            Button::new(
                "preview-back",
                "left",
                words!(
                    preview_back_five = "Back five seconds",
                    note = "A film preview's button."
                ),
                "ui.preview_seek",
            )
            .args(serde_json::json!({"delta":-5}))
            .enabled(current > 0.),
        ));
        bar.push(Item::widget(Slot::Navigate, 1, move |_, _| {
            div()
                .flex_none()
                .text_body()
                .whitespace_nowrap()
                .child(time.clone())
                .into_any_element()
        }));
        bar.push(Item::button(
            Slot::Navigate,
            2,
            Button::new(
                "preview-forward",
                "right",
                words!(
                    preview_forward_five = "Forward five seconds",
                    note = "A film preview's button."
                ),
                "ui.preview_seek",
            )
            .args(serde_json::json!({"delta":5}))
            .enabled(duration.is_none_or(|d| current < (d - 0.1).max(0.))),
        ));
    }
    if image {
        zoom_tools(&mut bar, shell);
    }
    if shell.walks_pictures() {
        picture_tools(&mut bar, shell);
    }
    // How the file is shown, at the bar's end (`look_tools`). A picture
    // written as text (an SVG) has a source, as any rendered text does: the
    // same button, off while the picture is shown. Refused to stay quick:
    // all of it, asked, and lit while so (a picture decoded whole, or past
    // even Full preview's bounds). With nothing else shown nor to be shown
    // (a failure, a file without a preview), the bytes are the one way left
    // to look: a button of its own.
    // Lit by what is shown (`PreviewState::shown`), never by a request on
    // its way: the bar changes once, with the picture.
    let whole = shell.preview.shown.whole;
    crate::preview_bar::look_tools(
        &mut bar,
        Looks {
            source: (image && shell.preview.picture_source.is_some()).then_some(false),
            turn: None,
            whole: (whole || matches!(shell.preview.content, Some(PreviewContent::TooLarge(_))))
                .then_some(Whole {
                    on: whole,
                    enabled: true,
                }),
            hex: hex_available(shell).then(|| {
                if shows_nothing(shell) {
                    Hex::Button
                } else {
                    Hex::Row
                }
            }),
            ways: crate::preview_bar::ways(shell, false, None),
        },
    );
    let (text, muted) = (caption(shell), theme.text_muted);
    bar.push(Item::widget(
        Slot::View,
        if image { 0 } else { 2 },
        move |_, _| {
            div()
                .min_w_0()
                .text_small()
                .text_color(muted)
                .truncate()
                .child(text.clone())
                .into_any_element()
        },
    ));
    crate::preview_bar::fullscreen_tool(&mut bar, shell, theme);
    crate::preview_bar::render(
        &bar,
        crate::preview::active_popup(shell, false),
        shell,
        theme,
        cx,
    )
}

/// A picture's way through the pane's pictures. On the whole screen: the
/// one before, the slideshow, the one after, and the slideshow's pace; in
/// the panel, the slideshow alone, in More (`ui.slideshow` goes to the
/// whole screen).
pub(crate) fn picture_tools(bar: &mut Bar, shell: &Shell) {
    let Some(fullscreen) = shell.preview.fullscreen.as_ref() else {
        let slideshow = words!(
            preview_slideshow = "Slideshow",
            note = "A picture preview's menu row: show the folder's pictures one after another, on the whole screen."
        );
        bar.push(Item::menu_only(
            Button::new(
                "preview-slideshow",
                "play",
                slideshow.clone(),
                "ui.slideshow",
            )
            .label(slideshow),
        ));
        return;
    };
    let (previous, next) = shell.preview.neighbours;
    let step = |id, icon, title, delta: i64, there: bool| {
        Button::new(id, icon, title, "ui.fullscreen_step")
            .args(serde_json::json!({"delta":delta}))
            .enabled(there)
    };
    bar.push(Item::button(
        Slot::Navigate,
        2,
        step(
            "picture-previous",
            "left",
            words!(
                preview_previous_picture = "Previous picture",
                note = "A button of a picture shown on the whole screen."
            ),
            -1,
            previous.is_some(),
        ),
    ));
    let playing = fullscreen.slideshow.is_some();
    bar.push(Item::button(
        Slot::Navigate,
        2,
        Button::new(
            "picture-slideshow",
            if playing { "pause" } else { "play" },
            if playing {
                words!(
                    preview_slideshow_pause = "Pause the slideshow",
                    note = "A button of a picture shown on the whole screen."
                )
            } else {
                words!(
                    preview_slideshow_play = "Play the slideshow",
                    note = "A button of a picture shown on the whole screen: show the pictures one after another."
                )
            },
            "ui.slideshow",
        )
        .active(playing),
    ));
    bar.push(Item::button(
        Slot::Navigate,
        2,
        step(
            "picture-next",
            "right",
            words!(
                preview_next_picture = "Next picture",
                note = "A button of a picture shown on the whole screen."
            ),
            1,
            next.is_some(),
        ),
    ));
    let seconds = crate::settings::slideshow_seconds(shell);
    bar.push(
        Item::button(
            Slot::Navigate,
            1,
            Button::new(
                "picture-pace",
                "chevron",
                each_picture_for(),
                "ui.preview_menu",
            )
            .args(serde_json::json!({"kind": crate::fullscreen::SLIDESHOW_POPUP}))
            .label(words!(
                preview_slideshow_pace = "{ $seconds }\u{a0}s",
                seconds = seconds,
                note = "On the button choosing a slideshow's pace: how long it shows each picture, in seconds, short (5 s)."
            )),
        )
        .opens(crate::fullscreen::SLIDESHOW_POPUP),
    );
    bar.popups.push((
        crate::fullscreen::SLIDESHOW_POPUP,
        std::rc::Rc::new(pace_popup),
    ));
}

/// The slideshow's pace's button and its popup's title, before how long.
fn each_picture_for() -> skwir_core::words::Words {
    words!(
        preview_each_picture_for = "Each picture for",
        note = "The button and the title of the choices of how long a slideshow shows each picture (5 s), which follow it."
    )
}

/// The slideshow's pace, a picker of the Reading page's choices.
fn pace_popup(shell: &Shell, theme: &Theme, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let popup = shell.preview.menu.as_ref()?;
    let chosen = crate::settings::slideshow_seconds(shell).to_string();
    let mut parts = vec![toolbar::MenuPart::Title(
        each_picture_for().say().into(),
        None,
    )];
    for (value, label) in crate::settings::slideshow_paces() {
        let seconds: u64 = value.parse().unwrap_or(5);
        parts.push(toolbar::MenuPart::Row(
            Button::new(
                format!("pace-{value}"),
                "",
                label.clone(),
                "ui.slideshow_pace",
            )
            .args(serde_json::json!({ "seconds": seconds }))
            .label(label)
            .active(chosen == value)
            .menu(),
        ));
    }
    Some(
        toolbar::menu_parts(
            toolbar::menu_panel(popup, "ui.preview_menu", theme, cx),
            parts,
            shell,
            theme,
            cx,
        )
        .into_any_element(),
    )
}
