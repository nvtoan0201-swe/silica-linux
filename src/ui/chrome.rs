//! The window frame Silica draws itself: rounded corners with a shadow, the frosted
//! background and resizing from the edges.

use gpui::{
    AnyElement, App, Bounds, CursorStyle, Decorations, HitboxBehavior, InteractiveElement,
    IntoElement, MouseButton, ParentElement, Pixels, Point, ResizeEdge, Size, StatefulInteractiveElement, Styled, Window,
    canvas, div, size, linear_color_stop, linear_gradient, point, prelude::FluentBuilder, px,
};

use crate::settings::Settings;
use crate::theme::{self, alpha};
use crate::ui::icon_view;

/// The window's corner radius; the editor card's outer corners are concentric with it.
pub const WINDOW_RADIUS: f32 = 16.;
/// Room around the window for its shadow, where the edges can be grabbed to resize.
const SHADOW: f32 = 12.;

/// Wraps a window's content in the frame.
pub fn frame(window: &mut Window, cx: &App, content: AnyElement) -> impl IntoElement {
    let colors = theme::colors(cx);
    let decorations = window.window_decorations();
    let translucent = Settings::get(cx).translucent_window;
    let (top, bottom) = if translucent {
        (alpha(colors.window_top, 0.78), alpha(colors.window_bottom, 0.82))
    } else {
        (colors.window_top, colors.window_bottom)
    };
    let background = linear_gradient(180., linear_color_stop(top, 0.), linear_color_stop(bottom, 1.));

    match decorations {
        Decorations::Server => {
            window.set_client_inset(px(0.));
            div().size_full().bg(background).child(content).into_any_element()
        }
        Decorations::Client { tiling } => {
            let inset = if tiling.is_tiled() { 0. } else { SHADOW };
            window.set_client_inset(px(inset));
            let radius = px(WINDOW_RADIUS);
            div()
                .id("window-backdrop")
                .size_full()
                .bg(gpui::transparent_black())
                .child(
                    // One hitbox per edge and corner, each with its own resize cursor, so the
                    // cursor changes without redrawing the window on every mouse move.
                    canvas(
                        |bounds, window, _| {
                            let edge = px(SHADOW);
                            let corner = px(SHADOW * 2.);
                            let (x, y) = (bounds.origin.x, bounds.origin.y);
                            let (w, h) = (bounds.size.width, bounds.size.height);
                            let rects = [
                                (CursorStyle::ResizeUpLeftDownRight, Bounds::new(point(x, y), size(corner, corner))),
                                (CursorStyle::ResizeUpRightDownLeft, Bounds::new(point(x + w - corner, y), size(corner, corner))),
                                (CursorStyle::ResizeUpRightDownLeft, Bounds::new(point(x, y + h - corner), size(corner, corner))),
                                (CursorStyle::ResizeUpLeftDownRight, Bounds::new(point(x + w - corner, y + h - corner), size(corner, corner))),
                                (CursorStyle::ResizeUpDown, Bounds::new(point(x, y), size(w, edge))),
                                (CursorStyle::ResizeUpDown, Bounds::new(point(x, y + h - edge), size(w, edge))),
                                (CursorStyle::ResizeLeftRight, Bounds::new(point(x, y), size(edge, h))),
                                (CursorStyle::ResizeLeftRight, Bounds::new(point(x + w - edge, y), size(edge, h))),
                            ];
                            rects.map(|(style, rect)| (style, window.insert_hitbox(rect, HitboxBehavior::Normal)))
                        },
                        |_, hitboxes, window, _| {
                            for (style, hitbox) in &hitboxes {
                                window.set_cursor_style(*style, hitbox);
                            }
                        },
                    )
                    .size_full()
                    .absolute(),
                )
                .when(!tiling.top, |d| d.pt(px(inset)))
                .when(!tiling.bottom, |d| d.pb(px(inset)))
                .when(!tiling.left, |d| d.pl(px(inset)))
                .when(!tiling.right, |d| d.pr(px(inset)))
                .on_mouse_down(MouseButton::Left, move |e, window, _| {
                    let size = window.window_bounds().get_bounds().size;
                    if let Some(edge) = resize_edge(e.position, px(SHADOW), size) {
                        window.start_window_resize(edge);
                    }
                })
                .child(
                    div()
                        .id("window")
                        .size_full()
                        .cursor(CursorStyle::Arrow)
                        .overflow_hidden()
                        .bg(background)
                        .border_1()
                        .border_color(if colors.dark { alpha(gpui::white(), 0.12) } else { alpha(gpui::black(), 0.14) })
                        .when(!(tiling.top || tiling.left), |d| d.rounded_tl(radius))
                        .when(!(tiling.top || tiling.right), |d| d.rounded_tr(radius))
                        .when(!(tiling.bottom || tiling.left), |d| d.rounded_bl(radius))
                        .when(!(tiling.bottom || tiling.right), |d| d.rounded_br(radius))
                        .when(!tiling.is_tiled(), |d| {
                            d.shadow(vec![gpui::BoxShadow {
                                color: alpha(gpui::black(), if colors.dark { 0.5 } else { 0.28 }),
                                blur_radius: px(SHADOW * 0.9),
                                spread_radius: px(0.),
                                offset: point(px(0.), px(3.)),
                            }])
                        })
                        .on_mouse_move(|_, _, cx| cx.stop_propagation())
                        .child(content),
                )
                .into_any_element()
        }
    }
}

fn resize_edge(pos: Point<Pixels>, inset: Pixels, size: Size<Pixels>) -> Option<ResizeEdge> {
    let corner = inset * 2.;
    let edge = if pos.y < inset && pos.x < corner || pos.x < inset && pos.y < corner {
        ResizeEdge::TopLeft
    } else if pos.y < inset && pos.x > size.width - corner || pos.x > size.width - inset && pos.y < corner {
        ResizeEdge::TopRight
    } else if pos.y < inset {
        ResizeEdge::Top
    } else if pos.y > size.height - inset && pos.x < corner || pos.x < inset && pos.y > size.height - corner {
        ResizeEdge::BottomLeft
    } else if pos.y > size.height - inset && pos.x > size.width - corner
        || pos.x > size.width - inset && pos.y > size.height - corner
    {
        ResizeEdge::BottomRight
    } else if pos.y > size.height - inset {
        ResizeEdge::Bottom
    } else if pos.x < inset {
        ResizeEdge::Left
    } else if pos.x > size.width - inset {
        ResizeEdge::Right
    } else {
        return None;
    };
    Some(edge)
}

/// Makes an element drag the window, double-click to maximize, and right-click for
/// the window menu, as a title bar does.
pub fn drag_area<E: InteractiveElement>(element: E) -> E {
    element
        .on_mouse_down(MouseButton::Left, |event, window, _| {
            if event.click_count == 2 {
                window.zoom_window();
            } else {
                window.start_window_move();
            }
        })
        .on_mouse_down(MouseButton::Right, |event, window, _| {
            window.show_window_menu(event.position);
        })
}

/// What the traffic lights do; the window decides how to close (it may ask to save).
pub struct TrafficLightActions {
    pub close: Box<dyn Fn(&mut Window, &mut App)>,
}

/// The close, minimize and zoom buttons, drawn as on macOS: colored circles that show
/// their symbols when the pointer is over the group.
pub fn traffic_lights(window: &Window, cx: &App, actions: TrafficLightActions) -> impl IntoElement {
    let colors = theme::colors(cx);
    let active = window.is_window_active();
    let light = |id: &'static str, color: gpui::Hsla, glyph: &'static str| {
        div()
            .id(id)
            .size(px(12.))
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(if active { color } else { colors.traffic_inactive })
            .border_1()
            .border_color(alpha(gpui::black(), 0.12))
            .child(
                div()
                    .invisible()
                    .group_hover("traffic-lights", |s| s.visible())
                    .child(icon_view(glyph, 9., alpha(gpui::black(), 0.6))),
            )
            .on_mouse_down(MouseButton::Left, |_, window, cx| {
                window.prevent_default();
                cx.stop_propagation();
            })
    };
    let close = actions.close;
    div()
        .group("traffic-lights")
        .flex()
        .flex_none()
        .items_center()
        .gap(px(8.))
        .child(light("close", colors.traffic_close, "x").on_click(move |_, window, cx| close(window, cx)))
        .child(light("minimize", colors.traffic_minimize, "minus").on_click(|_, window, _| window.minimize_window()))
        .child(light("zoom", colors.traffic_zoom, "plus").on_click(|_, window, _| window.zoom_window()))
}
