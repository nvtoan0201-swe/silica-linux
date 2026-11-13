//! Context menus: a small popover of commands at the pointer.

use std::rc::Rc;

use gpui::{
    App, Context, DismissEvent, EventEmitter, FocusHandle, Focusable, InteractiveElement,
    IntoElement, KeyBinding, MouseButton, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Window, actions, div, prelude::FluentBuilder, px,
};

use crate::theme::{self, alpha};

actions!(menu, [Dismiss, SelectNext, SelectPrevious, Confirm]);

type Handler = Rc<dyn Fn(&mut Window, &mut App)>;

pub enum MenuItem {
    Action { label: SharedString, enabled: bool, handler: Handler },
    Separator,
}

impl MenuItem {
    pub fn new(label: impl Into<SharedString>, handler: impl Fn(&mut Window, &mut App) + 'static) -> MenuItem {
        MenuItem::Action { label: label.into(), enabled: true, handler: Rc::new(handler) }
    }

    pub fn disabled(label: impl Into<SharedString>) -> MenuItem {
        MenuItem::Action { label: label.into(), enabled: false, handler: Rc::new(|_, _| {}) }
    }
}

pub struct ContextMenu {
    items: Vec<MenuItem>,
    focus_handle: FocusHandle,
    highlighted: Option<usize>,
}

impl EventEmitter<DismissEvent> for ContextMenu {}

impl Focusable for ContextMenu {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl ContextMenu {
    pub fn new(items: Vec<MenuItem>, window: &mut Window, cx: &mut Context<Self>) -> ContextMenu {
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle);
        ContextMenu { items, focus_handle, highlighted: None }
    }

    fn step(&mut self, forward: bool, cx: &mut Context<Self>) {
        let enabled: Vec<usize> = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| matches!(item, MenuItem::Action { enabled: true, .. }))
            .map(|(i, _)| i)
            .collect();
        if enabled.is_empty() {
            return;
        }
        let position = self.highlighted.and_then(|h| enabled.iter().position(|i| *i == h));
        let next = match (position, forward) {
            (None, true) => 0,
            (None, false) => enabled.len() - 1,
            (Some(p), true) => (p + 1) % enabled.len(),
            (Some(p), false) => (p + enabled.len() - 1) % enabled.len(),
        };
        self.highlighted = Some(enabled[next]);
        cx.notify();
    }

    fn run(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(MenuItem::Action { enabled: true, handler, .. }) = self.items.get(index) {
            let handler = handler.clone();
            cx.emit(DismissEvent);
            handler(window, cx);
        }
    }
}

impl Render for ContextMenu {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        let highlighted = self.highlighted;
        div()
            .id("context-menu")
            .key_context("Menu")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|_, _: &Dismiss, _, cx| cx.emit(DismissEvent)))
            .on_action(cx.listener(|this, _: &SelectNext, _, cx| this.step(true, cx)))
            .on_action(cx.listener(|this, _: &SelectPrevious, _, cx| this.step(false, cx)))
            .on_action(cx.listener(|this, _: &Confirm, window, cx| {
                if let Some(index) = this.highlighted {
                    this.run(index, window, cx);
                }
            }))
            .on_mouse_down_out(cx.listener(|_, _, _, cx| cx.emit(DismissEvent)))
            .min_w(px(200.))
            .p(px(5.))
            .flex()
            .flex_col()
            .bg(colors.popover)
            .border_1()
            .border_color(colors.popover_border)
            .rounded(px(10.))
            .shadow(vec![gpui::BoxShadow {
                color: alpha(gpui::black(), if colors.dark { 0.45 } else { 0.18 }),
                offset: gpui::point(px(0.), px(6.)),
                blur_radius: px(18.),
                spread_radius: px(0.),
            }])
            .text_size(px(13.))
            .children(self.items.iter().enumerate().map(|(index, item)| match item {
                MenuItem::Separator => div()
                    .mx(px(8.))
                    .my(px(4.))
                    .h(px(1.))
                    .bg(colors.separator)
                    .into_any_element(),
                MenuItem::Action { label, enabled, .. } => {
                    let enabled = *enabled;
                    div()
                        .id(index)
                        .px(px(9.))
                        .h(px(24.))
                        .flex()
                        .items_center()
                        .rounded(px(6.))
                        .text_color(if enabled { colors.label } else { colors.tertiary })
                        .when(enabled && highlighted == Some(index), |d| d.bg(colors.accent).text_color(gpui::white()))
                        .when(enabled, |d| d.hover(|s| s.bg(colors.accent).text_color(gpui::white())))
                        .child(label.clone())
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(move |this, _, window, cx| this.run(index, window, cx)))
                        .into_any_element()
                }
            }))
    }
}

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("escape", Dismiss, Some("Menu")),
        KeyBinding::new("down", SelectNext, Some("Menu")),
        KeyBinding::new("up", SelectPrevious, Some("Menu")),
        KeyBinding::new("enter", Confirm, Some("Menu")),
    ]);
}
