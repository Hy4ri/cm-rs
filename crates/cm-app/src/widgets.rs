//! Small styled building blocks.

use std::path::PathBuf;

use gpui::{AnyElement, Div, ElementId, FontWeight, ObjectFit, SharedString, Stateful, div, img, prelude::*, px};

use crate::theme;

/// A fixed-height frame that always shows the whole image, centered.
///
/// GPUI's `img` derives its size from the image aspect ratio, so a
/// `size_full` image in a wide box grows taller than the box and the bottom
/// is clipped. Pinning the image height and capping its width avoids that;
/// `Contain` letterboxes when the width cap kicks in. Callers set the width.
pub fn picture(path: Option<PathBuf>, height: f32) -> Div {
    div()
        .h(px(height))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .overflow_hidden()
        .rounded_sm()
        // Previews carry their own dark backdrop; only empty frames get a fill.
        .when(path.is_none(), |d| d.bg(theme::bg()))
        .when_some(path, |d, p| d.child(img(p).h(px(height)).max_w_full().rounded_sm().object_fit(ObjectFit::Contain)))
}

/// Label column + control, so every setting lines up on the same grid.
pub fn setting_row(label: impl Into<SharedString>, control: impl IntoElement) -> Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .min_h(px(28.))
        .child(div().w(px(120.)).flex_shrink_0().text_sm().text_color(theme::muted()).child(label.into()))
        .child(control)
}

/// Two or more mutually exclusive chips drawn as one control.
pub fn segmented(items: Vec<Stateful<Div>>) -> Div {
    div().flex().gap_1().children(items.into_iter().map(|i| i.into_any_element()).collect::<Vec<AnyElement>>())
}

pub fn button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Stateful<Div> {
    div()
        .id(id)
        .px_3()
        .py_1()
        .rounded_sm()
        .border_1()
        .border_color(theme::border())
        .bg(theme::panel())
        .text_sm()
        .cursor_pointer()
        .hover(|s| s.bg(theme::panel_hover()))
        .child(label.into())
}

pub fn primary_button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Stateful<Div> {
    div()
        .id(id)
        .px_6()
        .py_2()
        .rounded_sm()
        .bg(theme::accent())
        .text_color(gpui::black())
        .font_weight(FontWeight::BOLD)
        .cursor_pointer()
        .hover(|s| s.bg(theme::accent_hover()))
        .child(label.into())
}

pub fn chip(id: impl Into<ElementId>, label: impl Into<SharedString>, active: bool) -> Stateful<Div> {
    div()
        .id(id)
        .px_3()
        .py_1()
        .rounded_sm()
        .text_sm()
        .cursor_pointer()
        .border_1()
        .when(active, |d| d.bg(theme::accent()).text_color(gpui::black()).border_color(theme::accent()))
        .when(!active, |d| d.border_color(theme::border()).hover(|s| s.bg(theme::panel_hover())))
        .child(label.into())
}

pub fn section_title(text: impl Into<SharedString>) -> Div {
    div()
        .text_xs()
        .text_color(theme::muted())
        .font_weight(FontWeight::BOLD)
        .mt_2()
        .child(text.into().to_uppercase())
}

pub fn field(label: impl Into<SharedString>, value: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .gap_2()
        .text_sm()
        .child(div().w(px(110.)).flex_shrink_0().text_color(theme::muted()).child(label.into()))
        .child(div().child(value.into()))
}

pub fn panel() -> Div {
    div().bg(theme::panel()).rounded_md().border_1().border_color(theme::border()).p_3()
}
