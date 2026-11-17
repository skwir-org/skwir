//! A 3D model's preview (`InspectionKind::Model`, ADR-067): the mesh the
//! `models` plugin read whole for a window that draws meshes, turned and
//! drawn by `elements::model`.
//!
//! It is an inspection like any other, so Properties and the way back out
//! of a container are the frame's. What is its own: the model takes the
//! pointer (drag to turn it, Shift or the middle button to move it, the
//! wheel to zoom) and, once clicked, the keys (arrows turn it, Shift and the
//! arrows move it, the zoom keys zoom it, Ctrl+0 shows it as it opened).
//! The view belongs to the file shown: another file opens as its still.
use std::sync::Arc;

use gpui::{AnyElement, Context, MouseButton, div, prelude::*};

use crate::elements::model::{self, Handles, Model};
use crate::inspector::Display;
use crate::preview::PreviewContent;
use crate::preview_bar::{Bar, Item, Slot, label};
use crate::shell::Shell;
use crate::theme::Theme;
use crate::toolbar::Button;

/// Degrees a key turns the model.
const TURN_STEP: f32 = 15.;
/// Pixels a key moves the view, as it moves a map.
const PAN_STEP: f32 = 120.;
/// How much a zoom key zooms: as a notch of the wheel.
const ZOOM_STEP: f32 = std::f32::consts::SQRT_2;

/// The model's view, kept with the preview (`PreviewState::model`).
pub type ModelState = Handles;

pub fn content(shell: &Shell, display: &Display, theme: &Theme) -> AnyElement {
    let Some(shown) = display.model.clone() else {
        return div()
            .flex_1()
            .p_4()
            .text_color(theme.text_muted)
            .child(
                skwir_core::words!(
                    model_unreadable = "The model could not be read",
                    note = "In the preview of a 3D model whose file could not be read."
                )
                .say(),
            )
            .into_any_element();
    };
    let focus = shell.preview.inspection.focus.clone();
    // A figure to assistive technology, named by the file, saying what it
    // holds and what its keys do.
    let said = crate::preview::said(shell).map(|(name, _)| {
        (
            skwir_core::words!(
                model_said = "3D model: { $name }",
                name = name.to_string(),
                note = "What a screen reader calls the preview of a 3D model. name: its file's name."
            )
            .say(),
            skwir_core::words!(
                model_said_holds = "{ $count ->
                    [one] 1 triangle
                   *[other] { $count } triangles
                }. Arrows turn it, Shift and the arrows move it, the zoom keys zoom it, Ctrl+0 shows it as it opened",
                count = shown.mesh.triangles(),
                note = "What a screen reader hears of the preview of a 3D model: how many triangles its shape has, then what its keys do."
            )
            .say(),
        )
    });
    div()
        .id("preview-model-frame")
        .role(gpui::Role::Figure)
        .when_some(said, |d, (name, holds)| {
            d.aria_label(name).aria_description(holds)
        })
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .key_context(crate::keymap::MODEL)
        .when_some(focus, |d, focus| {
            let clicked = focus.clone();
            d.track_focus(&focus)
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    window.focus(&clicked, cx);
                })
                .on_mouse_down(MouseButton::Middle, {
                    let clicked = focus.clone();
                    move |_, window, cx| window.focus(&clicked, cx)
                })
        })
        .child(div().flex_1().min_h_0().child(model::model(
            "preview-model",
            shown,
            shell.preview.model.clone(),
            shell.prefs.model_look,
            theme.text_muted,
        )))
        .into_any_element()
}

/// The model's tools, in the bar's View slot: zoom out, in, and as it
/// opened; then how it is drawn, the first to go to More.
pub fn tools(bar: &mut Bar, shell: &Shell) {
    let opened = shell.preview.model.camera.get().is_none();
    let step = |id, icon, title, dir| {
        Button::new(id, icon, title, "ui.preview_zoom").args(serde_json::json!({ "dir": dir }))
    };
    bar.push(Item::button(
        Slot::View,
        1,
        step("model-zoom-out", "minus", label::zoom_out(), "out"),
    ));
    bar.push(Item::button(
        Slot::View,
        1,
        step("model-zoom-in", "plus", label::zoom_in(), "in"),
    ));
    bar.push(Item::button(
        Slot::View,
        2,
        step("model-fit", "fit", label::fit(), "reset").active(opened),
    ));
    let look = shell.prefs.model_look;
    let perspective = skwir_core::words!(
        model_perspective = "Perspective",
        note = "A 3D model preview's button: draw it in perspective, the far smaller than the near, or not."
    );
    bar.push(Item::button(
        Slot::View,
        0,
        Button::new(
            "model-perspective",
            "perspective",
            perspective.clone(),
            "ui.model_perspective",
        )
        .label(perspective)
        .active(look.perspective),
    ));
    let smooth = skwir_core::words!(
        model_smooth = "Smooth shading",
        note = "A 3D model preview's button: shade its faces smoothly rather than each flat."
    );
    bar.push(Item::button(
        Slot::View,
        0,
        Button::new("model-smooth", "smooth", smooth.clone(), "ui.model_smooth")
            .label(smooth)
            .active(look.smooth),
    ));
}

impl Shell {
    /// The model shown in the preview, if one is.
    fn model_shown(&self) -> Option<&Arc<Model>> {
        match &self.preview.content {
            Some(PreviewContent::Inspection(display)) => display.model.as_ref(),
            _ => None,
        }
    }

    /// The camera a key moves from: the one chosen, else how it opened.
    fn model_camera(&self) -> Option<(Arc<Model>, model::Camera)> {
        let shown = self.model_shown()?;
        let camera = self
            .preview
            .model
            .camera
            .get()
            .unwrap_or_else(|| shown.opening());
        Some((Arc::clone(shown), camera))
    }

    /// `ui.preview_zoom` on a model: `in` and `out` by a step around the
    /// middle, `reset` shows it as it opened. False when no model is shown.
    pub(crate) fn model_zoom(&mut self, direction: &str, cx: &mut Context<Self>) -> bool {
        let Some((shown, camera)) = self.model_camera() else {
            return false;
        };
        let factor = match direction {
            "in" => ZOOM_STEP,
            "out" => 1. / ZOOM_STEP,
            _ => {
                self.preview.model.camera.set(None);
                cx.notify();
                return true;
            }
        };
        if let Some(size) = self.preview.model.shown.get() {
            self.preview
                .model
                .camera
                .set(Some(shown.zoomed_at(camera, factor, [0., 0.], size)));
            cx.notify();
        }
        true
    }

    /// `ui.model_turn { dx, dy }` (the arrows): the view goes round the
    /// model by steps of `TURN_STEP` degrees, right and down.
    pub(crate) fn model_turn(&mut self, dx: f32, dy: f32, cx: &mut Context<Self>) {
        if let Some((_, camera)) = self.model_camera() {
            self.preview
                .model
                .camera
                .set(Some(camera.turned(dx * TURN_STEP, -dy * TURN_STEP)));
            cx.notify();
        }
    }

    /// `ui.model_pan { dx, dy }` (Shift and the arrows): the view moves by
    /// steps of `PAN_STEP` pixels, as a map's does.
    pub(crate) fn model_pan(&mut self, dx: f32, dy: f32, cx: &mut Context<Self>) {
        let Some((shown, camera)) = self.model_camera() else {
            return;
        };
        if let Some(size) = self.preview.model.shown.get() {
            self.preview.model.camera.set(Some(shown.stepped(
                camera,
                [dx * PAN_STEP, dy * PAN_STEP],
                size,
            )));
            cx.notify();
        }
    }
}
