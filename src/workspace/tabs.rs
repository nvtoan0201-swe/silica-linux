//! The tab bar on the title bar row: a scrolling row of tabs that can be dragged into
//! a new order, closed with their × or the middle button, and given a context menu.

use std::path::PathBuf;

use gpui::{
    AppContext, Context, InteractiveElement, IntoElement, MouseButton, ParentElement, Render,
    StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder, px,
};

use super::Workspace;
use crate::buffer::Buffer;
use crate::theme::{self, alpha};
use crate::ui::menu::MenuItem;
use crate::ui::{file_icon, icon_view};

/// A tab being dragged to a new place.
#[derive(Clone)]
pub struct DraggedTab {
    pub path: PathBuf,
    pub name: String,
    pub icon: &'static str,
    pub tint: gpui::Hsla,
}

impl Render for DraggedTab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .pl(px(10.))
            .pr(px(10.))
            .py(px(6.))
            .rounded(px(8.))
            .bg(colors.card)
            .border_1()
            .border_color(colors.card_border)
            .shadow_md()
            .text_size(px(12.))
            .text_color(colors.label)
            .child(icon_view(self.icon, 12., self.tint))
            .child(self.name.clone())
    }
}

impl Workspace {
    pub(super) fn render_tabs(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        let entity = cx.entity();
        let tabs: Vec<(usize, PathBuf)> = self.tabs.iter().cloned().enumerate().collect();
        div()
            .id("tab-bar")
            .flex()
            .flex_row()
            .items_center()
            .gap(px(6.))
            .px(px(2.))
            .py(px(4.))
            .min_w_0()
            .overflow_x_scroll()
            .track_scroll(&self.tab_scroll)
            .children(tabs.into_iter().filter_map(|(index, path)| {
                let buffer = self.buffers.get(&path)?;
                let is_active = self.active.as_ref() == Some(&path);
                Some(self.render_tab(index, &path, buffer, is_active, &entity, &colors, cx))
            }))
    }

    #[allow(clippy::too_many_arguments)]
    fn render_tab(
        &self,
        index: usize,
        path: &PathBuf,
        buffer: &Buffer,
        is_active: bool,
        entity: &gpui::Entity<Workspace>,
        colors: &theme::Colors,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let name = buffer.name();
        let is_dirty = buffer.is_dirty(cx);
        let missing = buffer.missing_on_disk;
        let (icon, tint) = file_icon::style(path, false, false, colors.dark);
        let group = format!("tab-{index}");
        let dragged = DraggedTab { path: path.clone(), name: name.clone(), icon, tint };
        let select = {
            let (entity, path) = (entity.clone(), path.clone());
            move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut gpui::App| {
                entity.update(cx, |this, cx| {
                    this.activate(&path, cx);
                    this.focus_active_editor(window, cx);
                });
            }
        };
        let close = {
            let (entity, path) = (entity.clone(), path.clone());
            move |window: &mut Window, cx: &mut gpui::App| {
                entity.update(cx, |this, cx| this.close_tab(&path, window, cx));
            }
        };
        let close_click = close.clone_box();
        let menu = {
            let (entity, path) = (entity.clone(), path.clone());
            let untitled = buffer.untitled;
            move |event: &gpui::MouseDownEvent, window: &mut Window, cx: &mut gpui::App| {
                let position = event.position;
                entity.update(cx, |this, cx| {
                    let items = this.tab_menu(&path, untitled, cx);
                    this.show_context_menu(position, items, window, cx);
                });
            }
        };
        let drop_target = path.clone();
        let drop_entity = entity.clone();
        let label_color = if is_active { colors.label } else { colors.secondary };

        div()
            .id(("tab", index))
            .group(group.clone())
            .flex()
            .flex_none()
            .items_center()
            .gap(px(6.))
            .pl(px(10.))
            .pr(px(6.))
            .py(px(6.))
            .rounded(px(8.))
            .text_size(px(12.))
            .text_color(label_color)
            // Only the active tab sits on glass.
            .when(is_active, |d| {
                d.bg(colors.capsule).border_1().border_color(colors.capsule_border).shadow(vec![gpui::BoxShadow {
                    color: alpha(gpui::black(), if colors.dark { 0.25 } else { 0.06 }),
                    offset: gpui::point(px(0.), px(1.)),
                    blur_radius: px(3.),
                    spread_radius: px(0.),
                }])
            })
            .when(!is_active, |d| d.border_1().border_color(gpui::transparent_black()))
            .child(icon_view(icon, 12., tint))
            .child(
                div()
                    .whitespace_nowrap()
                    .when(missing, |d| d.line_through())
                    .child(name),
            )
            .child(
                div()
                    .size(px(16.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(if is_dirty {
                        // The dirty dot, which turns into the close button under the pointer.
                        div()
                            .size(px(16.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                div()
                                    .size(px(7.))
                                    .rounded_full()
                                    .bg(alpha(colors.primary, 0.7))
                                    .group_hover(group.clone(), |s| s.invisible()),
                            )
                            .into_any_element()
                    } else {
                        div().into_any_element()
                    })
                    .child(
                        div()
                            .id(("close", index))
                            .absolute()
                            .size(px(16.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .when(!is_active, |d| d.invisible())
                            .when(is_dirty, |d| d.invisible())
                            .group_hover(group.clone(), |s| s.visible())
                            .hover(|s| s.bg(alpha(colors.primary, 0.1)))
                            .child(icon_view("x", 11., colors.secondary))
                            .on_mouse_down(MouseButton::Left, |_, window, cx| {
                                window.prevent_default();
                                cx.stop_propagation();
                            })
                            .on_click(move |_, window, cx| {
                                cx.stop_propagation();
                                close_click(window, cx)
                            }),
                    )
                    .relative(),
            )
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(select)
            // The middle button closes a tab, as browsers and other editors do.
            .on_mouse_up(MouseButton::Middle, move |_, window, cx| close(window, cx))
            .on_mouse_down(MouseButton::Right, menu)
            .on_drag(dragged, |tab, _, _, cx| cx.new(|_| tab.clone()))
            .drag_over::<DraggedTab>(move |style, _, _, _| style.bg(alpha(gpui::blue(), 0.08)))
            .on_drop(move |tab: &DraggedTab, _, cx| {
                drop_entity.update(cx, |this, cx| {
                    if let Some(to) = this.tabs.iter().position(|t| *t == drop_target) {
                        this.move_tab(&tab.path, to, cx);
                        this.activate(&tab.path, cx);
                    }
                });
            })
    }

    fn tab_menu(&self, path: &PathBuf, untitled: bool, cx: &mut Context<Self>) -> Vec<MenuItem> {
        let this = cx.entity().downgrade();
        let make = |f: fn(&mut Workspace, &PathBuf, &mut Window, &mut Context<Workspace>)| {
            let this = this.clone();
            let path = path.clone();
            move |window: &mut Window, cx: &mut gpui::App| {
                this.update(cx, |this, cx| f(this, &path, window, cx)).ok();
            }
        };
        let mut items = vec![
            MenuItem::new("Close", make(|this, path, window, cx| this.close_tab(path, window, cx))),
            MenuItem::new("Close Others", make(|this, path, window, cx| this.close_other_tabs(path, window, cx))),
        ];
        if !untitled {
            items.push(MenuItem::Separator);
            if self.contains(path) {
                items.push(MenuItem::new("Reveal in Explorer", make(|this, path, _, cx| this.reveal_in_tree(path, cx))));
            }
            let reveal = path.clone();
            items.push(MenuItem::new("Show in Files", move |_, cx| cx.reveal_path(&reveal)));
            let absolute = path.display().to_string();
            items.push(MenuItem::new("Copy Path", move |_, cx| {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(absolute.clone()))
            }));
        }
        items
    }
}

trait CloneBox {
    fn clone_box(&self) -> Box<dyn Fn(&mut Window, &mut gpui::App)>;
}

impl<F: Fn(&mut Window, &mut gpui::App) + Clone + 'static> CloneBox for F {
    fn clone_box(&self) -> Box<dyn Fn(&mut Window, &mut gpui::App)> {
        Box::new(self.clone())
    }
}
