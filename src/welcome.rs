//! The welcome window: the app icon with Open Project, and the recent projects.

use std::time::{SystemTime, UNIX_EPOCH};

use gpui::{
    AppContext, Context, ExternalPaths, InteractiveElement, IntoElement, MouseButton, ParentElement, Render,
    StatefulInteractiveElement, Styled, Window, div, img, prelude::FluentBuilder, px,
};

use crate::app;
use crate::project::recent::{self, RecentProject};
use crate::theme::{self, alpha};
use crate::ui::chrome::{self, TrafficLightActions};
use crate::ui::menu::{ContextMenu, MenuItem};
use crate::ui::{icon_view, push_button, unavailable};

pub struct Welcome {
    projects: Vec<RecentProject>,
    context_menu: Option<(gpui::Point<gpui::Pixels>, gpui::Entity<ContextMenu>, gpui::Subscription)>,
}

impl Welcome {
    pub fn new(window: &mut Window, _cx: &mut Context<Self>) -> Welcome {
        window.set_window_title("Welcome to Silica");
        Welcome { projects: recent::load(), context_menu: None }
    }

    fn show_menu(&mut self, position: gpui::Point<gpui::Pixels>, project: &RecentProject, window: &mut Window, cx: &mut Context<Self>) {
        let open = project.path.clone();
        let reveal = project.path.clone();
        let copy = project.path.display().to_string();
        let remove = project.path.clone();
        let this = cx.entity().downgrade();
        let items = vec![
            MenuItem::new("Open", move |_, cx| app::open_project(&open, None, cx)),
            MenuItem::new("Show in Files", move |_, cx| cx.reveal_path(&reveal)),
            MenuItem::new("Copy Path", move |_, cx| cx.write_to_clipboard(gpui::ClipboardItem::new_string(copy.clone()))),
            MenuItem::Separator,
            MenuItem::new("Remove from Recents", move |_, cx| {
                let projects = recent::remove(&remove);
                this.update(cx, |this, cx| {
                    this.projects = projects;
                    cx.notify();
                })
                .ok();
            }),
        ];
        let menu = cx.new(|cx| ContextMenu::new(items, window, cx));
        let subscription = cx.subscribe(&menu, |this, _, _: &gpui::DismissEvent, cx| {
            this.context_menu = None;
            cx.notify();
        });
        self.context_menu = Some((position, menu, subscription));
        cx.notify();
    }
}

/// "5 minutes ago", "yesterday", as macOS writes relative dates.
fn relative_time(seconds: u64) -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let ago = now.saturating_sub(seconds);
    let plural = |n: u64, unit: &str| if n == 1 { format!("1 {unit} ago") } else { format!("{n} {unit}s ago") };
    match ago {
        0..60 => "now".into(),
        60..3600 => plural(ago / 60, "minute"),
        3600..86400 => plural(ago / 3600, "hour"),
        86400..172800 => "yesterday".into(),
        172800..604800 => plural(ago / 86400, "day"),
        604800..2_592_000 => plural(ago / 604800, "week"),
        2_592_000..31_536_000 => plural(ago / 2_592_000, "month"),
        _ => plural(ago / 31_536_000, "year"),
    }
}

impl Render for Welcome {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        let last = self.projects.first().cloned();
        let traffic = chrome::traffic_lights(
            window,
            cx,
            TrafficLightActions { close: Box::new(|window, _| window.remove_window()) },
        );

        let hero = div()
            .w(px(330.))
            .h_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .px(px(32.))
            .child(img(crate::assets::app_icon()).size(px(128.)).rounded(px(28.)))
            .child(div().mt(px(10.)).text_size(px(30.)).font_weight(gpui::FontWeight::BOLD).child("Silica"))
            .child(
                div()
                    .mt(px(4.))
                    .text_size(px(13.))
                    .text_color(colors.secondary)
                    .child(format!("Version {}", env!("CARGO_PKG_VERSION"))),
            )
            .child(
                div()
                    .mt(px(28.))
                    .w(px(230.))
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(
                        push_button("open-project", "Open Project…", true, cx, |_, _, cx| app::choose_and_open(cx))
                            .h(px(36.))
                            .w_full(),
                    )
                    .when_some(last, |d, last| {
                        let path = last.path.clone();
                        d.child(
                            push_button("reopen", format!("Reopen “{}”", last.name), false, cx, move |_, _, cx| {
                                app::open_project(&path, None, cx)
                            })
                            .h(px(36.))
                            .w_full()
                            .overflow_hidden(),
                        )
                    }),
            )
            .child(
                div()
                    .mt(px(14.))
                    .text_size(px(11.))
                    .text_color(colors.tertiary)
                    .child("or drop a project folder anywhere in this window"),
            );

        let recents = div()
            .flex_1()
            .h_full()
            .py(px(18.))
            .pr(px(18.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .px(px(10.))
                    .pt(px(6.))
                    .flex()
                    .items_center()
                    .child(div().flex_1().font_weight(gpui::FontWeight::BOLD).child("Recent Projects"))
                    .when(!self.projects.is_empty(), |d| {
                        d.child(
                            div()
                                .id("clear")
                                .text_size(px(12.))
                                .text_color(colors.secondary)
                                .cursor_pointer()
                                .child("Clear")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    recent::clear();
                                    this.projects.clear();
                                    cx.notify();
                                })),
                        )
                    }),
            )
            .child(if self.projects.is_empty() {
                unavailable("clock", "No Recent Projects", "Projects you open will show up here.", None, cx)
                    .into_any_element()
            } else {
                div()
                    .id("recent-list")
                    .flex_1()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .children(self.projects.clone().into_iter().enumerate().map(|(index, project)| {
                        let group = format!("recent-{index}");
                        let open = project.path.clone();
                        let remove = project.path.clone();
                        let menu_project = project.clone();
                        div()
                            .id(index)
                            .group(group.clone())
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .px(px(10.))
                            .py(px(8.))
                            .rounded(px(12.))
                            .hover(|s| s.bg(alpha(colors.primary, 0.07)))
                            .child(
                                div()
                                    .w(px(30.))
                                    .flex()
                                    .justify_center()
                                    .child(icon_view("folder-fill", 24., crate::theme::hex(if colors.dark { 0x5AA5F5 } else { 0x3B8EEA }))),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    .child(
                                        div()
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .whitespace_nowrap()
                                            .overflow_hidden()
                                            .text_ellipsis()
                                            .child(project.name.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.))
                                            .text_color(colors.secondary)
                                            .whitespace_nowrap()
                                            .overflow_hidden()
                                            .text_ellipsis()
                                            .child(project.display_path()),
                                    ),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(px(11.))
                                    .text_color(colors.tertiary)
                                    .group_hover(group.clone(), |s| s.invisible())
                                    .child(relative_time(project.last_opened)),
                            )
                            .child(
                                div()
                                    .id(("remove", index))
                                    .absolute()
                                    .right(px(10.))
                                    .invisible()
                                    .group_hover(group, |s| s.visible())
                                    .child(icon_view("circle-x", 15., colors.secondary))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        this.projects = recent::remove(&remove);
                                        cx.notify();
                                    })),
                            )
                            .relative()
                            .on_click(move |_, _, cx| app::open_project(&open, None, cx))
                            .on_mouse_down(MouseButton::Right, cx.listener(move |this, event: &gpui::MouseDownEvent, window, cx| {
                                this.show_menu(event.position, &menu_project, window, cx);
                            }))
                    }))
                    .into_any_element()
            });

        let content = div()
            .id("welcome")
            .size_full()
            .relative()
            .flex()
            .text_color(colors.label)
            .font_family(crate::desktop::fonts().ui.clone())
            .text_size(px(13.))
            .child(hero)
            .child(recents)
            .child(chrome::drag_area(div().id("welcome-title").absolute().top_0().left_0().w(px(330.)).h(px(36.))))
            .child(div().absolute().top(px(12.)).left(px(16.)).child(traffic))
            .drag_over::<ExternalPaths>(move |style, _, _, _| style.border_2().border_color(colors.accent).rounded(px(16.)))
            .on_drop(|paths: &ExternalPaths, _, cx| {
                // A folder opens as a project; a file in the project it belongs to.
                let paths = paths.paths().to_vec();
                app::open_paths(&paths, cx);
            })
            .when_some(self.context_menu.as_ref().map(|(p, m, _)| (*p, m.clone())), |d, (position, menu)| {
                d.child(gpui::deferred(gpui::anchored().position(position).child(menu)).with_priority(1))
            });
        chrome::frame(window, cx, content.into_any_element())
    }
}
