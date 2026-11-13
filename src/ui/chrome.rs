//! The window frame Silica draws itself: rounded corners with a shadow, the frosted
//! background, resizing from the edges and the macOS-style traffic lights.

use gpui::{
    AnyElement, App, Bounds, CursorStyle, Decorations, HitboxBehavior, InteractiveElement,
    IntoElement, MouseButton, ParentElement, Pixels, Point, ResizeEdge, Size, Styled, Window,
    canvas, div, linear_color_stop, linear_gradient, point, prelude::FluentBuilder, px,
};

use crate::settings::Settings;
use crate::theme::{self, alpha};

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
                    canvas(
                        |_bounds, window, _| {
                            window.insert_hitbox(
                                Bounds::new(point(px(0.), px(0.)), window.window_bounds().get_bounds().size),
                                HitboxBehavior::Normal,
                            )
                        },
                        move |_bounds, hitbox, window, _| {
                            let mouse = window.mouse_position();
                            let size = window.window_bounds().get_bounds().size;
                            let Some(edge) = resize_edge(mouse, px(SHADOW), size) else { return };
                            window.set_cursor_style(
                                match edge {
                                    ResizeEdge::Top | ResizeEdge::Bottom => CursorStyle::ResizeUpDown,
                                    ResizeEdge::Left | ResizeEdge::Right => CursorStyle::ResizeLeftRight,
                                    ResizeEdge::TopLeft | ResizeEdge::BottomRight => CursorStyle::ResizeUpLeftDownRight,
                                    ResizeEdge::TopRight | ResizeEdge::BottomLeft => CursorStyle::ResizeUpRightDownLeft,
                                },
                                &hitbox,
                            );
                        },
                    )
                    .size_full()
                    .absolute(),
                )
                .when(!tiling.top, |d| d.pt(px(inset)))
                .when(!tiling.bottom, |d| d.pb(px(inset)))
                .when(!tiling.left, |d| d.pl(px(inset)))
                .when(!tiling.right, |d| d.pr(px(inset)))
                .on_mouse_move(|_, window, _| window.refresh())
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
