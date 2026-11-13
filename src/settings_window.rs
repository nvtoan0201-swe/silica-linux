//! Settings (Ctrl+,), laid out like System Settings: a sidebar of panes on the left
//! and the chosen pane's grouped form on the right. Changes apply to open windows at once.

use std::sync::OnceLock;

use gpui::{
    AnyElement, App, AppContext, Bounds, ClickEvent, Context, DismissEvent, Entity, Hsla,
    InteractiveElement, IntoElement, MouseButton, ParentElement, Pixels, Point, Render,
    SharedString, StatefulInteractiveElement, Styled, StyledText, Subscription, TitlebarOptions,
    Window, WindowBackgroundAppearance, WindowBounds, WindowDecorations, WindowOptions, anchored,
    deferred, div, img, prelude::FluentBuilder, px, size,
};

use crate::settings::{Appearance, Settings};
use crate::syntax::{Language, SyntaxTheme};
use crate::theme::{self, alpha, hex};
use crate::ui::chrome;
use crate::ui::icon_view;
use crate::ui::menu::{ContextMenu, MenuItem};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Pane {
    General,
    FontsAndColors,
    Editor,
    About,
}

impl Pane {
    fn title(self) -> &'static str {
        match self {
            Pane::General => "General",
            Pane::FontsAndColors => "Fonts & Colors",
            Pane::Editor => "Editor",
            Pane::About => "About",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Pane::General => "settings",
            Pane::FontsAndColors => "type",
            Pane::Editor => "text-cursor",
            Pane::About => "info",
        }
    }

    fn tint(self) -> u32 {
        match self {
            Pane::General | Pane::About => 0x8E8E93,
            Pane::FontsAndColors => 0x007AFF,
            Pane::Editor => 0xFF9500,
        }
    }
}

pub struct SettingsWindow {
    pane: Pane,
    menu: Option<(Point<Pixels>, Entity<ContextMenu>, Subscription)>,
}

pub fn show(cx: &mut App) {
    if let Some(handle) = cx.windows().into_iter().find_map(|w| w.downcast::<SettingsWindow>()) {
        let _ = handle.update(cx, |_, window, _| window.activate_window());
        return;
    }
    let bounds = Bounds::centered(None, size(px(760.), px(560.)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions { title: Some("Settings".into()), appears_transparent: true, traffic_light_position: None }),
        window_decorations: Some(WindowDecorations::Client),
        window_background: WindowBackgroundAppearance::Transparent,
        window_min_size: Some(size(px(620.), px(460.))),
        app_id: Some("silica".into()),
        ..Default::default()
    };
    let _ = cx.open_window(options, |window, cx| {
        window.set_window_title("Settings");
        cx.new(|_| SettingsWindow { pane: Pane::General, menu: None })
    });
}

/// Installed monospaced font families, for the font picker.
fn monospaced_families() -> &'static [String] {
    static FAMILIES: OnceLock<Vec<String>> = OnceLock::new();
    FAMILIES.get_or_init(|| {
        let output = std::process::Command::new("fc-list").args([":spacing=mono", "family"]).output();
        let mut families: Vec<String> = output
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .unwrap_or_default()
            .lines()
            .filter_map(|line| line.split(',').next().map(|f| f.trim().to_string()))
            .filter(|f| !f.is_empty() && !f.contains("Emoji") && !f.contains("SignWriting"))
            .collect();
        families.sort_by_key(|f| f.to_lowercase());
        families.dedup();
        families
    })
}

impl SettingsWindow {
    fn show_menu(&mut self, position: Point<Pixels>, items: Vec<MenuItem>, window: &mut Window, cx: &mut Context<Self>) {
        let menu = cx.new(|cx| ContextMenu::new(items, window, cx));
        let subscription = cx.subscribe(&menu, |this, _, _: &DismissEvent, cx| {
            this.menu = None;
            cx.notify();
        });
        self.menu = Some((position, menu, subscription));
        cx.notify();
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        let item = |pane: Pane, cx: &mut Context<Self>| {
            let selected = pane == self.pane;
            div()
                .id(pane.title())
                .h(px(36.))
                .px(px(6.))
                .flex()
                .items_center()
                .gap(px(10.))
                .rounded(px(9.))
                .when(selected, |d| d.bg(alpha(colors.primary, 0.07)))
                .child(
                    div()
                        .size(px(22.))
                        .rounded(px(5.5))
                        .bg(hex(pane.tint()))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon_view(pane.icon(), 13., gpui::white())),
                )
                .child(
                    div()
                        .text_size(px(14.))
                        .when(selected, |d| d.font_weight(gpui::FontWeight::SEMIBOLD))
                        .child(pane.title()),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.pane = pane;
                    cx.notify();
                }))
        };
        div()
            .w(px(215.))
            .h_full()
            .flex_none()
            .bg(if colors.dark { hex(0x262628) } else { hex(0xF9F9FA) })
            .px(px(10.))
            .pt(px(50.))
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(item(Pane::General, cx))
                    .child(item(Pane::FontsAndColors, cx))
                    .child(item(Pane::Editor, cx)),
            )
            .child(item(Pane::About, cx))
    }

    fn detail(&mut self, cx: &mut Context<Self>) -> AnyElement {
        match self.pane {
            Pane::General => self.general(cx).into_any_element(),
            Pane::FontsAndColors => self.fonts_and_colors(cx).into_any_element(),
            Pane::Editor => self.editor_pane(cx).into_any_element(),
            Pane::About => about(cx).into_any_element(),
        }
    }

    /// Appearance, and how projects open and show their files.
    fn general(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = Settings::get(cx).clone();
        form()
            .child(group(
                "Appearance",
                vec![
                    row(
                        "Appearance",
                        segmented(
                            "appearance",
                            &[("System", Appearance::System), ("Light", Appearance::Light), ("Dark", Appearance::Dark)],
                            settings.appearance,
                            |value, cx| Settings::update(cx, |s| s.appearance = value),
                            cx,
                        ),
                        cx,
                    ),
                    toggle_row(
                        "translucent",
                        "See-through window background",
                        settings.translucent_window,
                        |on, cx| Settings::update(cx, |s| s.translucent_window = on),
                        cx,
                    ),
                ],
                None,
                cx,
            ))
            .child(group(
                "Projects",
                vec![toggle_row(
                    "restore-tabs",
                    "Reopen a project’s tabs where you left them",
                    settings.restores_tabs,
                    |on, cx| Settings::update(cx, |s| s.restores_tabs = on),
                    cx,
                )],
                None,
                cx,
            ))
    }

    /// Editor font and syntax colors, with a live sample, and the sidebar's font size.
    fn fonts_and_colors(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        let settings = Settings::get(cx).clone();
        let family = settings.editor_font_family();
        let system_font = crate::desktop::fonts().monospace.clone();
        let font_label: SharedString = match &settings.font_family {
            Some(family) => family.clone().into(),
            None => format!("{system_font} (System)").into(),
        };
        let theme_label: SharedString = settings.syntax_theme.name().into();

        let font_picker = popup_button("font", font_label, cx, cx.listener(move |this, event: &ClickEvent, window, cx| {
            let current = Settings::get(cx).font_family.clone();
            let mut items = vec![MenuItem::check(format!("{system_font} (System)"), current.is_none(), |_, cx| {
                Settings::update(cx, |s| s.font_family = None)
            })];
            items.push(MenuItem::Separator);
            for family in monospaced_families() {
                let chosen = family.clone();
                items.push(MenuItem::check(family.clone(), current.as_deref() == Some(family), move |_, cx| {
                    let chosen = chosen.clone();
                    Settings::update(cx, |s| s.font_family = Some(chosen))
                }));
            }
            this.show_menu(event.position(), items, window, cx);
        }));

        let theme_picker = popup_button("syntax-theme", theme_label, cx, cx.listener(|this, event: &ClickEvent, window, cx| {
            let current = Settings::get(cx).syntax_theme;
            let items = SyntaxTheme::ALL
                .iter()
                .map(|theme| {
                    let theme = *theme;
                    MenuItem::check(theme.name(), theme == current, move |_, cx| {
                        Settings::update(cx, |s| s.syntax_theme = theme)
                    })
                })
                .collect();
            this.show_menu(event.position(), items, window, cx);
        }));

        // The sample, colored by the chosen theme the way the editor colors Swift.
        let indent = " ".repeat(settings.tab_width);
        let sample = format!(
            "// Says hello.\nfunc greet(_ name: String) -> String {{\n{indent}let count = 2\n{indent}return \"Hello, \\(name)!\"\n}}"
        );
        let highlights: Vec<_> = Language::Swift
            .grammar()
            .highlight(&sample)
            .into_iter()
            .map(|span| {
                (
                    span.range,
                    gpui::HighlightStyle { color: Some(hex(settings.syntax_theme.color(span.kind, colors.dark))), ..Default::default() },
                )
            })
            .collect();
        let sample_view = div()
            .w_full()
            .p(px(12.))
            .rounded(px(8.))
            .bg(colors.card)
            .border_1()
            .border_color(alpha(colors.primary, 0.06))
            .font_family(SharedString::from(family))
            .text_size(px(settings.font_size))
            .line_height(px((settings.font_size * 1.38).round()))
            .text_color(colors.label)
            .child(StyledText::new(sample).with_highlights(highlights));

        form()
            .child(group(
                "Font",
                vec![
                    div().w_full().py(px(3.)).child(sample_view).into_any_element(),
                    row("Font", font_picker, cx),
                    row(
                        "Size",
                        stepper("font-size", format!("{} pt", settings.font_size as i32), cx, |delta, cx| {
                            Settings::update(cx, |s| s.font_size += delta)
                        }),
                        cx,
                    ),
                ],
                None,
                cx,
            ))
            .child(group(
                "Sidebar",
                vec![row(
                    "Size",
                    stepper("sidebar-size", format!("{} pt", settings.sidebar_font_size as i32), cx, |delta, cx| {
                        Settings::update(cx, |s| s.sidebar_font_size += delta)
                    }),
                    cx,
                )],
                None,
                cx,
            ))
            .child(group("Colors", vec![row("Syntax colors", theme_picker, cx)], None, cx))
    }

    /// What the editor shows beside the text, and indentation for new files.
    fn editor_pane(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = Settings::get(cx).clone();
        let tab_label: SharedString = format!("{} spaces", settings.tab_width).into();
        let indent_label: SharedString = if settings.indents_with_tabs { "Tabs".into() } else { "Spaces".into() };
        let tab_picker = popup_button("tab-width", tab_label, cx, cx.listener(|this, event: &ClickEvent, window, cx| {
            let current = Settings::get(cx).tab_width;
            let items = Settings::TAB_WIDTHS
                .iter()
                .map(|width| {
                    let width = *width;
                    MenuItem::check(format!("{width} spaces"), width == current, move |_, cx| {
                        Settings::update(cx, |s| s.tab_width = width)
                    })
                })
                .collect();
            this.show_menu(event.position(), items, window, cx);
        }));
        let indent_picker = popup_button("indent-with", indent_label, cx, cx.listener(|this, event: &ClickEvent, window, cx| {
            let tabs = Settings::get(cx).indents_with_tabs;
            let items = vec![
                MenuItem::check("Spaces", !tabs, |_, cx| Settings::update(cx, |s| s.indents_with_tabs = false)),
                MenuItem::check("Tabs", tabs, |_, cx| Settings::update(cx, |s| s.indents_with_tabs = true)),
            ];
            this.show_menu(event.position(), items, window, cx);
        }));
        form()
            .child(group(
                "Display",
                vec![
                    toggle_row("line-numbers", "Show line numbers", settings.shows_line_numbers, |on, cx| {
                        Settings::update(cx, |s| s.shows_line_numbers = on)
                    }, cx),
                    toggle_row("indent-guides", "Show indent guides", settings.shows_indent_guides, |on, cx| {
                        Settings::update(cx, |s| s.shows_indent_guides = on)
                    }, cx),
                    toggle_row("minimap", "Show minimap", settings.shows_minimap, |on, cx| {
                        Settings::update(cx, |s| s.shows_minimap = on)
                    }, cx),
                    toggle_row("wrap", "Wrap long lines", settings.wraps_lines, |on, cx| {
                        Settings::update(cx, |s| s.wraps_lines = on)
                    }, cx),
                ],
                None,
                cx,
            ))
            .child(group(
                "Indentation",
                vec![row("Tab width", tab_picker, cx), row("Indent new files with", indent_picker, cx)],
                Some("Files that are already indented keep their own style."),
                cx,
            ))
    }
}

fn about(cx: &App) -> impl IntoElement {
    let colors = theme::colors(cx);
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(6.))
        .child(img(crate::assets::app_icon()).size(px(96.)).rounded(px(21.)))
        .child(div().mt(px(8.)).text_size(px(22.)).font_weight(gpui::FontWeight::BOLD).child("Silica"))
        .child(div().text_color(colors.secondary).child(format!("Version {}", env!("CARGO_PKG_VERSION"))))
        .child(
            div()
                .mt(px(12.))
                .max_w(px(380.))
                .text_center()
                .text_size(px(12.))
                .text_color(colors.secondary)
                .child("A lightweight code editor, made of glass. The Linux edition, built with Rust and GPUI."),
        )
        .child(
            div()
                .mt(px(4.))
                .text_size(px(11.))
                .text_color(colors.tertiary)
                .child("Icons from Lucide (ISC). Swift logo from Simple Icons (CC0)."),
        )
}

// MARK: - Form building blocks

/// A pane's scrolling column of groups, filling the window's width.
fn form() -> gpui::Stateful<gpui::Div> {
    div().id("form").size_full().overflow_y_scroll().p(px(20.)).pt(px(44.)).flex().flex_col().gap(px(24.))
}

/// A rounded box of rows split by hairlines, with a title above it and a note below it.
fn group(title: &str, rows: Vec<AnyElement>, footer: Option<&str>, cx: &App) -> impl IntoElement {
    let colors = theme::colors(cx);
    let count = rows.len();
    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .child(div().px(px(10.)).font_weight(gpui::FontWeight::SEMIBOLD).child(title.to_string()))
        .child(
            div()
                .flex()
                .flex_col()
                .rounded(px(12.))
                .bg(alpha(colors.primary, if colors.dark { 0.06 } else { 0.045 }))
                .children(rows.into_iter().enumerate().map(move |(index, content)| {
                    div()
                        .child(
                            div()
                                .px(px(10.))
                                .py(px(8.))
                                .min_h(px(38.))
                                .flex()
                                .items_center()
                                .child(content),
                        )
                        .when(index + 1 < count, |d| d.child(div().mx(px(10.)).h(px(1.)).bg(colors.separator)))
                })),
        )
        .when_some(footer, |d, footer| {
            d.child(div().px(px(10.)).text_size(px(11.)).text_color(colors.secondary).child(footer.to_string()))
        })
}

/// A row with its label on the left and its control pushed to the right edge.
fn row(label: &str, control: impl IntoElement, _cx: &App) -> AnyElement {
    div()
        .w_full()
        .flex()
        .items_center()
        .gap(px(16.))
        .child(div().flex_1().child(label.to_string()))
        .child(control)
        .into_any_element()
}

/// A switch, as GNOME and macOS draw them.
fn toggle_row(
    id: &'static str,
    label: &str,
    on: bool,
    change: impl Fn(bool, &mut App) + 'static,
    cx: &App,
) -> AnyElement {
    let colors = theme::colors(cx);
    let track: Hsla = if on { colors.accent } else { alpha(colors.primary, if colors.dark { 0.22 } else { 0.14 }) };
    let switch = div()
        .id(id)
        .w(px(34.))
        .h(px(20.))
        .rounded_full()
        .bg(track)
        .p(px(2.))
        .flex()
        .when(on, |d| d.justify_end())
        .child(div().size(px(16.)).rounded_full().bg(gpui::white()).shadow_sm())
        .on_click(move |_, _, cx| change(!on, cx));
    row(label, switch, cx)
}

/// A segmented control: one choice of a few, side by side.
fn segmented<T: Copy + PartialEq + 'static>(
    id: &'static str,
    options: &[(&'static str, T)],
    current: T,
    change: impl Fn(T, &mut App) + Clone + 'static,
    cx: &App,
) -> impl IntoElement {
    let colors = theme::colors(cx);
    div()
        .flex()
        .p(px(2.))
        .rounded(px(8.))
        .bg(alpha(colors.primary, if colors.dark { 0.1 } else { 0.07 }))
        .children(options.iter().enumerate().map(|(index, (label, value))| {
            let selected = *value == current;
            let value = *value;
            let change = change.clone();
            div()
                .id((id, index))
                .px(px(12.))
                .h(px(24.))
                .flex()
                .items_center()
                .rounded(px(6.))
                .text_size(px(12.))
                .when(selected, |d| d.bg(colors.capsule).shadow_sm().font_weight(gpui::FontWeight::MEDIUM))
                .child(*label)
                .on_click(move |_, _, cx| change(value, cx))
        }))
}

/// A pop-up button that shows a menu of choices.
fn popup_button(
    id: &'static str,
    label: SharedString,
    cx: &App,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let colors = theme::colors(cx);
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(6.))
        .pl(px(10.))
        .pr(px(6.))
        .h(px(24.))
        .rounded(px(6.))
        .bg(colors.capsule)
        .border_1()
        .border_color(colors.capsule_border)
        .shadow_sm()
        .text_size(px(13.))
        .child(label)
        .child(icon_view("chevrons-up-down", 12., colors.secondary))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(on_click)
}

/// A value with − and + buttons.
fn stepper(id: &'static str, value: String, cx: &App, change: impl Fn(f32, &mut App) + Clone + 'static) -> impl IntoElement {
    let colors = theme::colors(cx);
    let button = |index: usize, icon: &'static str, delta: f32| {
        let change = change.clone();
        div()
            .id((id, index))
            .w(px(26.))
            .h(px(22.))
            .flex()
            .items_center()
            .justify_center()
            .hover(|s| s.bg(alpha(colors.primary, 0.08)))
            .child(icon_view(icon, 12., colors.label))
            .on_click(move |_, _, cx| change(delta, cx))
    };
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(value)
        .child(
            div()
                .flex()
                .rounded(px(6.))
                .bg(colors.capsule)
                .border_1()
                .border_color(colors.capsule_border)
                .overflow_hidden()
                .child(button(0, "minus", -1.))
                .child(div().w(px(1.)).bg(colors.separator))
                .child(button(1, "plus", 1.)),
        )
}

impl Render for SettingsWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        let detail = self.detail(cx);
        let content = div()
            .id("settings")
            .size_full()
            .relative()
            .flex()
            .text_color(colors.label)
            .font_family(crate::desktop::fonts().ui.clone())
            .text_size(px(13.))
            .child(self.sidebar(cx))
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .bg(if colors.dark { hex(0x1F1F21) } else { hex(0xF1F1F3) })
                    .child(detail),
            )
            .child(chrome::drag_area(div().id("settings-title").absolute().top_0().left_0().right_0().h(px(32.))))
            .when_some(self.menu.as_ref().map(|(p, m, _)| (*p, m.clone())), |d, (position, menu)| {
                d.child(deferred(anchored().position(position).snap_to_window_with_margin(px(8.)).child(menu)).with_priority(1))
            });
        chrome::frame(window, cx, content.into_any_element())
    }
}
