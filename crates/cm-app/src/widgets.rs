//! Small styled building blocks.

use gpui::{Div, ElementId, FontWeight, SharedString, Stateful, div, prelude::*, px};

use crate::theme;

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
