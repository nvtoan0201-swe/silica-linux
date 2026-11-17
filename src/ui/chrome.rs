//! The window frame Silica draws itself: rounded corners with a shadow, the frosted
//! background and resizing from the edges.

use gpui::{
    AnyElement, App, Bounds, CursorStyle, Decorations, DispatchPhase, HitboxBehavior, InteractiveElement,
    IntoElement, MouseButton, MouseDownEvent, ParentElement, ResizeEdge, StatefulInteractiveElement, Styled, Window,
    canvas, div, size, linear_color_stop, linear_gradient, point, prelude::FluentBuilder, px,
};

use crate::settings::Settings;
use crate::theme::{self, alpha};
use crate::ui::icon_view;

/// The window's corner radius; the editor card's outer corners are concentric with it.
pub const WINDOW_RADIUS: f32 = 16.;
/// Room around the window for its shadow, where the edges can be grabbed to resize.
const SHADOW: f32 = 12.;
/// How far into the window, past its border, the edges can still be grabbed to resize.
const GRAB_INSIDE: f32 = 5.;

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
                .when(!tiling.top, |d| d.pt(px(inset)))
                .when(!tiling.bottom, |d| d.pb(px(inset)))
                .when(!tiling.left, |d| d.pl(px(inset)))
                .when(!tiling.right, |d| d.pr(px(inset)))
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
                .when(!tiling.is_tiled(), |d| d.child(resize_handles()))
                .into_any_element()
        }
    }
}

/// The edges and corners that resize the window: the shadow around it and a few pixels
/// of the window itself, so the visible border can be grabbed. Painted over the content,
/// so those pixels resize rather than reaching what's beneath, with a larger grab at the
/// rounded corners.
fn resize_handles() -> impl IntoElement {
    canvas(
        |bounds, window, _| {
            let edge = px(SHADOW + GRAB_INSIDE);
            let corner = px(SHADOW + WINDOW_RADIUS + GRAB_INSIDE);
            let (x, y) = (bounds.origin.x, bounds.origin.y);
            let (w, h) = (bounds.size.width, bounds.size.height);
            // Corners first: where they overlap an edge, the first hitbox wins.
            let handles = [
                (ResizeEdge::TopLeft, Bounds::new(point(x, y), size(corner, corner))),
                (ResizeEdge::TopRight, Bounds::new(point(x + w - corner, y), size(corner, corner))),
                (ResizeEdge::BottomLeft, Bounds::new(point(x, y + h - corner), size(corner, corner))),
                (ResizeEdge::BottomRight, Bounds::new(point(x + w - corner, y + h - corner), size(corner, corner))),
                (ResizeEdge::Top, Bounds::new(point(x, y), size(w, edge))),
                (ResizeEdge::Bottom, Bounds::new(point(x, y + h - edge), size(w, edge))),
                (ResizeEdge::Left, Bounds::new(point(x, y), size(edge, h))),
                (ResizeEdge::Right, Bounds::new(point(x + w - edge, y), size(edge, h))),
            ];
            // Later hitboxes are in front, so insert in reverse to put the corners on top.
            let mut hitboxes: Vec<_> = handles
                .into_iter()
                .rev()
                .map(|(edge, rect)| (edge, window.insert_hitbox(rect, HitboxBehavior::BlockMouse)))
                .collect();
            hitboxes.reverse();
            hitboxes
        },
        |_, hitboxes, window, _| {
            for (edge, hitbox) in &hitboxes {
                window.set_cursor_style(cursor_for(*edge), hitbox);
            }
            window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                if phase != DispatchPhase::Capture || event.button != MouseButton::Left {
                    return;
                }
                if let Some((edge, _)) = hitboxes.iter().find(|(_, hitbox)| hitbox.is_hovered(window)) {
                    window.start_window_resize(*edge);
                    cx.stop_propagation();
                }
            });
        },
    )
    .size_full()
    .absolute()
    .top_0()
    .left_0()
}

fn cursor_for(edge: ResizeEdge) -> CursorStyle {
    match edge {
        ResizeEdge::TopLeft | ResizeEdge::BottomRight => CursorStyle::ResizeUpLeftDownRight,
        ResizeEdge::TopRight | ResizeEdge::BottomLeft => CursorStyle::ResizeUpRightDownLeft,
        ResizeEdge::Top | ResizeEdge::Bottom => CursorStyle::ResizeUpDown,
        ResizeEdge::Left | ResizeEdge::Right => CursorStyle::ResizeLeftRight,
    }
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
