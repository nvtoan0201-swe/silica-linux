//! Small pieces of Silica's look shared by every window.

pub mod chrome;
pub mod file_icon;
pub mod menu;
pub mod text_field;

use gpui::{
    AnyElement, App, ClickEvent, ElementId, Hsla, InteractiveElement, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder, px, svg,
};

use crate::assets::icon;
use crate::theme::{self, alpha};

/// An SVG icon tinted with `color`.
pub fn icon_view(name: &str, size: f32, color: Hsla) -> gpui::Svg {
    svg().path(icon(name)).size(px(size)).flex_none().text_color(color)
}

/// A plain icon button for the title bar row that shows a soft rounded fill under the
/// pointer, darker while pressed, like the toolbar buttons of system apps.
pub fn title_bar_button(
    id: impl Into<ElementId>,
    icon_name: &str,
    color: Hsla,
    cx: &App,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> gpui::Stateful<gpui::Div> {
    let colors = theme::colors(cx);
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .w(px(26.))
        .h(px(22.))
        .rounded(px(6.))
        .hover(|s| s.bg(alpha(colors.primary, 0.08)))
        .active(|s| s.bg(alpha(colors.primary, 0.14)))
        .child(icon_view(icon_name, 15., color))
        .on_mouse_down(gpui::MouseButton::Left, |_, window, cx| {
            // Not a window drag from the title bar.
            window.prevent_default();
            cx.stop_propagation();
        })
        .on_click(on_click)
}

/// The glass capsule behind the status bar, the filter field and shortcut keys.
pub fn capsule(cx: &App) -> gpui::Div {
    let colors = theme::colors(cx);
    div()
        .bg(colors.capsule)
        .border_1()
        .border_color(colors.capsule_border)
        .rounded_full()
        .shadow(vec![gpui::BoxShadow {
            color: alpha(gpui::black(), if colors.dark { 0.25 } else { 0.06 }),
            offset: gpui::point(px(0.), px(1.)),
            blur_radius: px(3.),
            spread_radius: px(0.),
        }])
}

/// A centered message with an icon, title and description, as SwiftUI's
/// `ContentUnavailableView`.
pub fn unavailable(
    icon_name: &str,
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
    actions: Option<AnyElement>,
    cx: &App,
) -> gpui::Div {
    let colors = theme::colors(cx);
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(10.))
        .px(px(32.))
        .child(icon_view(icon_name, 40., colors.secondary).mb(px(4.)))
        .child(
            div()
                .text_size(px(20.))
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(colors.label)
                .child(title.into()),
        )
        .child(
            div()
                .text_size(px(13.))
                .text_color(colors.secondary)
                .max_w(px(420.))
                .text_center()
                .child(description.into()),
        )
        .when_some(actions, |this, actions| this.child(div().mt(px(8.)).child(actions)))
}

/// A push button in the style of a bordered macOS button; `prominent` is the
/// accent-colored default button.
pub fn push_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    prominent: bool,
    cx: &App,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> gpui::Stateful<gpui::Div> {
    let colors = theme::colors(cx);
    let (bg, fg) = if prominent {
        (colors.accent, gpui::white())
    } else {
        (alpha(colors.primary, if colors.dark { 0.14 } else { 0.07 }), colors.label)
    };
    div()
        .id(id)
        .px(px(14.))
        .h(px(28.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(bg)
        .text_color(fg)
        .text_size(px(13.))
        .font_weight(gpui::FontWeight::MEDIUM)
        .hover(|s| s.opacity(0.88))
        .active(|s| s.opacity(0.75))
        .child(label.into())
        .on_click(on_click)
}
