//! Common preview controls; document-only tools remain in the reader.
use crate::{
    elements::adaptive::AdaptiveRow,
    preview::PreviewContent,
    shell::Shell,
    theme::Theme,
    toolbar::{self, Button},
};
use bibli_core::preview::PreviewInfo;
use gpui::{AnyElement, Context, Div, div, prelude::*, px};

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

pub fn zoom_menu(shell: &Shell, fit_label: &'static str, cx: &mut Context<Shell>) -> Div {
    let mut menu = div().flex().flex_col().gap_1();
    for (id, icon, label, value) in [
        ("menu-fit", "fit", fit_label, "reset"),
        ("menu-width", "width", "Fit width", "width"),
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
                .render(shell, cx)
                .justify_start(),
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
    menu.child(presets).child(
        div()
            .flex()
            .gap_1()
            .child(zoom_step(false).render(shell, cx))
            .child(zoom_step(true).render(shell, cx)),
    )
}

fn zoom_step(increase: bool) -> Button {
    let (id, icon, title, value) = if increase {
        ("preview-zoom-in", "plus", "Zoom in", "in")
    } else {
        ("preview-zoom-out", "minus", "Zoom out", "out")
    };
    Button::new(id, icon, title, "ui.preview_zoom").args(serde_json::json!({"dir":value}))
}

fn open_button() -> Button {
    Button::new(
        "preview-external",
        "open",
        "Open in default application",
        "ui.preview_open_external",
    )
}

fn caption(shell: &Shell) -> String {
    match &shell.preview.content {
        Some(PreviewContent::Inspection(display)) => display.data.title.clone(),
        Some(PreviewContent::Image {
            width,
            height,
            detail: Some(PreviewInfo::Video { .. }),
            ..
        }) => format!("Video still · {width} × {height}"),
        Some(PreviewContent::Image { width, height, .. }) => format!("{width} × {height}"),
        Some(PreviewContent::Text { lines, truncated }) => format!(
            "{} lines{}",
            lines.len(),
            if *truncated { " · partial" } else { "" }
        ),
        Some(PreviewContent::Dir { items }) => {
            format!("{} items", crate::format::thousands(*items))
        }
        Some(PreviewContent::Error(_)) => "Preview unavailable".into(),
        Some(PreviewContent::Generic) => "No preview".into(),
        None => "Loading…".into(),
    }
}

fn row(
    shell: &Shell,
    mode: usize,
    anchor: &crate::elements::popover::PopoverAnchor,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let theme = cx.global::<Theme>().clone();
    let image = matches!(shell.preview.content, Some(PreviewContent::Image { .. }));
    let video = match &shell.preview.content {
        Some(PreviewContent::Image {
            detail: Some(PreviewInfo::Video { seconds, duration }),
            ..
        }) => Some((*seconds, *duration)),
        _ => None,
    };
    let mut row = toolbar::row(&theme).id(("preview-tools", mode));
    if let Some((seconds, duration)) = video {
        let current = shell.preview.seconds.unwrap_or(seconds);
        let time = if mode < 2 {
            duration.map_or_else(
                || format!("{current:.1}s"),
                |total| format!("{current:.1}s / {total:.1}s"),
            )
        } else {
            format!("{current:.1}s")
        };
        row = row
            .child(
                Button::new(
                    "preview-back",
                    "left",
                    "Back five seconds",
                    "ui.preview_seek",
                )
                .args(serde_json::json!({"delta":-5}))
                .enabled(current > 0.)
                .render(shell, cx),
            )
            .child(div().flex_none().text_xs().whitespace_nowrap().child(time))
            .child(
                Button::new(
                    "preview-forward",
                    "right",
                    "Forward five seconds",
                    "ui.preview_seek",
                )
                .args(serde_json::json!({"delta":5}))
                .enabled(duration.is_none_or(|d| current < (d - 0.1).max(0.)))
                .render(shell, cx),
            );
    } else if mode == 0 || !image {
        row = row.child(
            div()
                .when(image, |d| d.flex_none().whitespace_nowrap())
                .text_xs()
                .text_color(theme.text_muted)
                .when(!image, |d| d.min_w_0().flex_1().truncate())
                .child(caption(shell)),
        );
    }
    if image && (mode == 0 || video.is_none()) {
        if mode < 2 {
            row = row.child(zoom_step(false).render(shell, cx));
        }
        row = row.child(
            Button::new(
                "preview-zoom-menu",
                "chevron",
                "Zoom and fit options",
                "ui.preview_menu",
            )
            .args(serde_json::json!({"kind":"zoom"}))
            .label(zoom_label(shell))
            .anchor_if(
                anchor,
                mode == 0
                    || shell
                        .preview
                        .menu
                        .as_ref()
                        .is_some_and(|p| p.kind == "zoom"),
            )
            .render(shell, cx),
        );
        if mode < 2 {
            row = row.child(zoom_step(true).render(shell, cx));
        }
    }
    if image && mode < 2 {
        row = row.child(
            Button::new("preview-fit", "fit", "Fit whole image", "ui.preview_zoom")
                .args(serde_json::json!({"dir":"reset"}))
                .active(shell.preview.zoom.is_none() && !shell.preview.fit_width)
                .render(shell, cx),
        );
        if mode == 0 {
            row = row.child(
                Button::new(
                    "preview-width",
                    "width",
                    "Fit image width",
                    "ui.preview_zoom",
                )
                .args(serde_json::json!({"dir":"width"}))
                .active(shell.preview.fit_width)
                .render(shell, cx),
            );
        }
    }
    if image {
        row = row.child(div().flex_1().min_w_0());
    }
    if mode == 0 || !image {
        if shell.app.previews.ids().contains(&"preview-hex") {
            row = row.child(
                Button::new("preview-hex", "list", "View as hexadecimal", "ui.inspect")
                    .args(serde_json::json!({"action":"hex"}))
                    .render(shell, cx),
            );
        }
        row = row.child(open_button().render(shell, cx));
    } else {
        row = row.child(
            Button::new(
                "preview-more",
                "more",
                "More preview controls",
                "ui.preview_menu",
            )
            .active(shell.preview.menu.is_some())
            .anchor_if(
                anchor,
                video.is_some()
                    || shell
                        .preview
                        .menu
                        .as_ref()
                        .is_some_and(|p| p.kind != "zoom"),
            )
            .render(shell, cx),
        );
    }
    row.into_any_element()
}

pub fn render(shell: &Shell, theme: &Theme, cx: &mut Context<Shell>) -> AnyElement {
    let anchor = crate::elements::popover::PopoverAnchor::default();
    let mut root = div().relative().flex_none().child(AdaptiveRow {
        variants: (0..3).map(|mode| row(shell, mode, &anchor, cx)).collect(),
        height: 42.,
    });
    if let Some(popup) = &shell.preview.menu {
        let mut panel = toolbar::popup_panel(popup, "ui.preview_menu", theme, cx);
        if matches!(shell.preview.content, Some(PreviewContent::Image { .. })) {
            panel = panel.child(zoom_menu(shell, "Fit image", cx));
        }
        if popup.kind != "zoom" {
            panel = panel
                .child(div().h(px(1.)).my_1().bg(theme.border))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.text_muted)
                        .child(caption(shell)),
                )
                .child(
                    open_button()
                        .label("Open externally")
                        .render(shell, cx)
                        .justify_start(),
                );
        }
        root = root.child(anchor.popup(panel));
    }
    root.into_any_element()
}
