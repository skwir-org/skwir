//! Common preview controls; document-only tools remain in the reader.
use crate::{
    preview::PreviewContent,
    preview_bar::{Bar, Item, Slot, label},
    shell::Shell,
    theme::Theme,
    toolbar::{self, Button},
};
use gpui::{AnyElement, Context, Div, div, prelude::*};
use skwir_core::preview::PreviewInfo;

pub fn zoom_label(shell: &Shell) -> String {
    shell.preview.zoom.map_or_else(
        || {
            if shell.preview.fit_width {
                "Width".into()
            } else {
                "Fit".into()
            }
        },
        |zoom| format!("{:.0}%", zoom * 100.),
    )
}

/// The zoom button's own popup: a picker of values, like a combo box.
fn zoom_menu(shell: &Shell, cx: &mut Context<Shell>) -> Div {
    let mut menu = div().flex().flex_col().gap_1();
    for (id, icon, label, value) in [
        ("menu-fit", "fit", label::FIT, "reset"),
        ("menu-width", "width", label::FIT_WIDTH, "width"),
    ] {
        menu = menu.child(
            Button::new(id, icon, label, "ui.preview_zoom")
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
    for (id, label, value) in [
        ("zoom50", "50%", "0.5"),
        ("zoom75", "75%", "0.75"),
        ("zoom100", "100%", "1"),
        ("zoom150", "150%", "1.5"),
        ("zoom200", "200%", "2"),
        ("zoom400", "400%", "4"),
    ] {
        presets = presets.child(
            Button::new(id, "", label, "ui.preview_zoom")
                .label(label)
                .args(serde_json::json!({"dir":value}))
                .render(shell, cx),
        );
    }
    menu.child(presets)
}

/// Zoom, for whatever is looked at rather than read: a picture or a page.
/// The same five tools in the same order in both. A book laid out into pages
/// by its engine is read: its steps size its text (`Shell::preview_zoom`),
/// and say so.
pub fn zoom_tools(bar: &mut Bar, shell: &Shell) {
    let fit = shell.preview.zoom.is_none() && !shell.preview.fit_width;
    let step = |id, icon, title, dir| {
        Button::new(id, icon, title, "ui.preview_zoom").args(serde_json::json!({"dir":dir}))
    };
    let (smaller, larger) = if shell.reader_reflows() {
        ("Smaller text", "Larger text")
    } else {
        ("Zoom out", "Zoom in")
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
                label::ZOOM,
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
        step("preview-fit", "fit", label::FIT, "reset").active(fit),
    ));
    bar.push(Item::button(
        Slot::View,
        0,
        step("preview-width", "width", label::FIT_WIDTH, "width").active(shell.preview.fit_width),
    ));
    bar.popups.push((
        "zoom",
        std::rc::Rc::new(|shell, theme, cx| {
            let popup = shell.preview.menu.as_ref()?;
            Some(
                toolbar::popup_panel(popup, "ui.preview_menu", theme, cx)
                    .child(zoom_menu(shell, cx))
                    .into_any_element(),
            )
        }),
    ));
}

/// Whether the bytes of what is shown can be viewed: the hexadecimal
/// inspection provider is registered, and what is shown is a file here, not
/// a member asked for that was never written out (too large, refused). The
/// registry lock is taken only when its revision changed since the last
/// read, never once per variant per frame.
pub(crate) fn hex_available(shell: &Shell) -> bool {
    if shell.preview.descend.is_some() {
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

/// What the bar says about content with no bar of its own. An inspection
/// has its own (`inspector::toolbar`), so it never comes here.
fn caption(shell: &Shell) -> String {
    match &shell.preview.content {
        Some(PreviewContent::Image {
            width,
            height,
            detail: Some(PreviewInfo::Video { .. }),
            ..
        }) => format!("Video still · {width} × {height}"),
        Some(PreviewContent::Image { width, height, .. }) => format!("{width} × {height}"),
        Some(PreviewContent::Dir { items, .. }) => crate::format::count(*items, "item"),
        Some(PreviewContent::Error(_)) => "Preview unavailable".into(),
        Some(PreviewContent::Generic) => "No preview".into(),
        // Only once the wait has lasted: a caption that flashes by is noise.
        None if shell.preview_busy() => "Loading…".into(),
        None | Some(PreviewContent::Inspection(_)) => String::new(),
    }
}

pub fn render(shell: &Shell, theme: &Theme, cx: &mut Context<Shell>) -> AnyElement {
    let image = matches!(shell.preview.content, Some(PreviewContent::Image { .. }));
    let mut bar = Bar::new("preview-tools");
    crate::preview_bar::close_tool(&mut bar, shell, theme);
    if let Some(PreviewContent::Image {
        detail: Some(PreviewInfo::Video { seconds, duration }),
        ..
    }) = &shell.preview.content
    {
        let current = shell.preview.seconds.unwrap_or(*seconds);
        let time = duration.map_or_else(
            || format!("{current:.1}s"),
            |total| format!("{current:.1}s / {total:.1}s"),
        );
        bar.push(Item::button(
            Slot::Navigate,
            2,
            Button::new(
                "preview-back",
                "left",
                "Back five seconds",
                "ui.preview_seek",
            )
            .args(serde_json::json!({"delta":-5}))
            .enabled(current > 0.),
        ));
        bar.push(Item::widget(Slot::Navigate, 1, move |_, _| {
            div()
                .flex_none()
                .text_xs()
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
                "Forward five seconds",
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
    if hex_available(shell) {
        bar.push(Item::menu_only(
            Button::new("preview-hex", "grid", label::HEX, "ui.inspect")
                .args(serde_json::json!({"action":"hex"})),
        ));
    }
    let (text, muted) = (caption(shell), theme.text_muted);
    bar.push(Item::widget(
        Slot::View,
        if image { 0 } else { 2 },
        move |_, _| {
            div()
                .min_w_0()
                .text_xs()
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
fn picture_tools(bar: &mut Bar, shell: &Shell) {
    let Some(fullscreen) = shell.preview.fullscreen.as_ref() else {
        bar.push(Item::menu_only(
            Button::new("preview-slideshow", "play", "Slideshow", "ui.slideshow")
                .label("Slideshow"),
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
            "Previous picture",
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
                "Pause the slideshow"
            } else {
                "Play the slideshow"
            },
            "ui.slideshow",
        )
        .active(playing),
    ));
    bar.push(Item::button(
        Slot::Navigate,
        2,
        step("picture-next", "right", "Next picture", 1, next.is_some()),
    ));
    let seconds = crate::settings::slideshow_seconds(shell);
    bar.push(
        Item::button(
            Slot::Navigate,
            1,
            Button::new(
                "picture-pace",
                "chevron",
                "Each picture for",
                "ui.preview_menu",
            )
            .args(serde_json::json!({"kind": crate::fullscreen::SLIDESHOW_POPUP}))
            .label(format!("{seconds} s")),
        )
        .opens(crate::fullscreen::SLIDESHOW_POPUP),
    );
    bar.popups.push((
        crate::fullscreen::SLIDESHOW_POPUP,
        std::rc::Rc::new(pace_popup),
    ));
}

/// The slideshow's pace, a picker of the Reading page's choices.
fn pace_popup(shell: &Shell, theme: &Theme, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let popup = shell.preview.menu.as_ref()?;
    let chosen = crate::settings::slideshow_seconds(shell).to_string();
    let mut rows = div().flex().flex_col().gap_1().child(
        div()
            .px_2()
            .text_xs()
            .text_color(theme.text_muted)
            .child("Each picture for"),
    );
    for (value, label) in crate::settings::SLIDESHOW_PACES {
        let seconds: u64 = value.parse().unwrap_or(5);
        rows = rows.child(
            Button::new(format!("pace-{value}"), "", label, "ui.slideshow_pace")
                .args(serde_json::json!({ "seconds": seconds }))
                .label(label)
                .active(chosen == value)
                .menu()
                .render(shell, cx),
        );
    }
    Some(
        toolbar::popup_panel(popup, "ui.preview_menu", theme, cx)
            .child(rows)
            .into_any_element(),
    )
}
