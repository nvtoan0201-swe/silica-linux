//! The editor side of the window: the title bar row with the tabs, then the glass
//! card with the active file, its status bar, or the empty state.

use gpui::{
    Context, InteractiveElement, IntoElement, ObjectFit, ParentElement,
    StatefulInteractiveElement, Styled, StyledImage, Window, div, img, prelude::FluentBuilder, px,
};

use super::{CARD_INSET, CARD_RADIUS, GoToLine, TITLE_BAR_HEIGHT, Workspace};
use crate::buffer::{Content, format_bytes};
use crate::editor::MINIMAP_WIDTH;
use crate::preview::PreviewFormat;
use crate::settings::Settings;
use crate::theme::{self, alpha};
use crate::ui::chrome;
use crate::ui::{capsule, icon_view, push_button, title_bar_button, unavailable};

impl Workspace {
    pub(super) fn render_editor_area(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        let sidebar_visible = self.sidebar_visible;
        let preview = self
            .active_buffer()
            .filter(|b| b.editor().is_some())
            .and_then(|b| b.preview_format.map(|f| (f, b.showing_preview)));

        // Title bar row: the tabs, after the sidebar button when the sidebar is hidden.
        let title_row = chrome::drag_area(
            div()
                .id("editor-title")
                .h(px(TITLE_BAR_HEIGHT))
                .flex_none()
                .flex()
                .items_center()
                .gap(px(8.))
                .when(!sidebar_visible, |d| {
                    d.pl(px(12.)).child(title_bar_button("show-sidebar", "panel-left", colors.secondary, cx, |_, window, cx| {
                        window.dispatch_action(Box::new(super::ToggleSidebar), cx)
                    }))
                })
                .child(div().flex_1().min_w_0().flex().child(self.render_tabs(cx)))
                .when_some(preview, |d, (format, showing)| {
                    d.child(self.render_preview_toggle(format, showing, cx))
                }),
        );

        let card = div()
            .id("card")
            .flex_1()
            .min_h_0()
            .mr(px(CARD_INSET))
            .mb(px(CARD_INSET))
            .when(!sidebar_visible, |d| d.ml(px(CARD_INSET)))
            .rounded(px(CARD_RADIUS))
            .bg(colors.card)
            .border_1()
            .border_color(colors.card_border)
            .shadow(vec![gpui::BoxShadow {
                color: colors.card_shadow,
                offset: gpui::point(px(0.), px(2.)),
                blur_radius: px(10.),
                spread_radius: px(0.),
            }])
            .overflow_hidden()
            .child(self.render_card_content(window, cx));

        div()
            .flex_1()
            .min_w(px(420.))
            .h_full()
            .flex()
            .flex_col()
            .child(title_row)
            .child(card)
    }

    /// Source / Preview switch for Markdown, CSV and HTML tabs, in the quiet icon style of
    /// the other title bar buttons; the current mode is drawn in the primary color.
    fn render_preview_toggle(&self, format: PreviewFormat, showing: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        let option = |id: &'static str, icon: &'static str, on: bool, show: bool, cx: &mut Context<Self>| {
            div()
                .id(id)
                .w(px(26.))
                .h(px(22.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.))
                .when(on, |d| d.bg(alpha(colors.primary, 0.08)))
                .child(icon_view(icon, 15., if on { colors.label } else { colors.secondary }))
                .on_mouse_down(gpui::MouseButton::Left, |_, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                })
                .on_click(cx.listener(move |this, _, _, cx| this.set_preview(show, cx)))
        };
        let _ = format;
        div()
            .flex()
            .gap(px(2.))
            .mr(px(CARD_INSET + 4.))
            .child(option("show-source", "file-text", !showing, false, cx))
            .child(option("show-preview", "eye", showing, true, cx))
    }

    fn render_card_content(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let colors = theme::colors(cx);
        let Some(path) = self.active.clone() else { return self.render_empty(cx).into_any_element() };
        let Some(buffer) = self.buffers.get(&path) else { return self.render_empty(cx).into_any_element() };
        let name = buffer.name();
        match &buffer.content {
            Content::Text(editor) => {
                if buffer.showing_preview {
                    if let Some(format) = buffer.preview_format {
                        return crate::preview::render(format, &path, editor.clone(), window, cx);
                    }
                }
                // The status bar floats over the text's bottom corner rather than taking
                // a strip of the card, left of the minimap when it's shown.
                let minimap = if Settings::get(cx).shows_minimap { MINIMAP_WIDTH } else { 0. };
                div()
                    .size_full()
                    .relative()
                    .child(editor.clone())
                    .child(
                        div()
                            .absolute()
                            .bottom(px(4.))
                            .right(px(4. + minimap))
                            .child(self.render_status_bar(cx)),
                    )
                    .into_any_element()
            }
            Content::Image(image) => {
                let details = format!(
                    "{} × {} px  ·  {}  ·  {}",
                    image.width,
                    image.height,
                    image.format,
                    format_bytes(image.byte_count)
                );
                let (width, height) = (image.width as f32, image.height as f32);
                div()
                    .size_full()
                    .p(px(24.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .w_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                // Fitted to the card, never enlarged past one image pixel per point.
                                div()
                                    .max_w(px(width))
                                    .max_h(px(height))
                                    .size_full()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(
                                        img(path.clone())
                                            .max_w(px(width))
                                            .max_h(px(height))
                                            .size_full()
                                            .object_fit(ObjectFit::Contain),
                                    ),
                            ),
                    )
                    .child(div().text_size(px(13.)).text_color(colors.secondary).child(details))
                    .into_any_element()
            }
            Content::Unsupported(kind) => {
                let open = path.clone();
                let title = match *kind {
                    "Video" => "Can’t Play Video",
                    "Audio" => "Can’t Play Audio",
                    _ => "Can’t Show PDF",
                };
                unavailable(
                    if *kind == "PDF" { "file-text" } else { "film" },
                    title,
                    format!("Silica doesn’t show {} files. “{name}” can be opened in another app.", kind.to_lowercase()),
                    Some(
                        push_button("open-with", "Open With Default App", true, cx, move |_, _, cx| {
                            cx.open_with_system(&open)
                        })
                        .into_any_element(),
                    ),
                    cx,
                )
                .into_any_element()
            }
            Content::Binary => unavailable(
                "file-question",
                "Binary File",
                format!("“{name}” isn’t a text file, so it can’t be shown in the editor."),
                None,
                cx,
            )
            .into_any_element(),
            Content::TooLarge(size) => unavailable(
                "triangle-alert",
                "File Too Large",
                format!("“{name}” is {}, which is too large to open here.", format_bytes(*size)),
                None,
                cx,
            )
            .into_any_element(),
            Content::Unreadable(message) => {
                unavailable("triangle-alert", "Can’t Read File", message.clone(), None, cx).into_any_element()
            }
        }
    }

    /// Cursor position, indentation, encoding, line endings and language, in a small
    /// line over the bottom-right corner of the editor.
    fn render_status_bar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        let Some(buffer) = self.active_buffer() else { return div().into_any_element() };
        let Some(editor) = buffer.editor().cloned() else { return div().into_any_element() };
        let status = editor.read(cx).cursor_status(cx);
        let indentation = editor.update(cx, |e, cx| e.indentation(cx));
        let buffer = self.active_buffer().unwrap();
        let position = if status.selected > 0 {
            format!("Ln {}, Col {} ({} selected)", status.line, status.column, status.selected)
        } else {
            format!("Ln {}, Col {}", status.line, status.column)
        };
        let indent = if indentation.uses_tabs { "Tabs".to_string() } else { format!("Spaces: {}", indentation.width) };
        let language = buffer.language().map_or("Plain Text", |l| l.display_name());
        capsule(cx)
            .flex()
            .items_center()
            .gap(px(12.))
            .px(px(10.))
            .py(px(4.))
            .text_size(px(11.))
            .text_color(colors.secondary)
            .whitespace_nowrap()
            .child(
                div()
                    .id("status-position")
                    .cursor_pointer()
                    .child(position)
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(GoToLine), cx)),
            )
            .child(indent)
            .child(buffer.encoding.name())
            .child(buffer.line_ending_name(cx))
            .child(language)
            .into_any_element()
    }

    fn render_empty(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        let shortcut = |title: &'static str, keys: &'static str| {
            div()
                .flex()
                .items_center()
                .gap(px(16.))
                .child(div().w(px(170.)).flex().justify_end().text_color(colors.secondary).child(title))
                .child(
                    capsule(cx)
                        .px(px(8.))
                        .py(px(2.))
                        .text_size(px(12.))
                        .font_family(crate::desktop::fonts().monospace.clone())
                        .child(keys),
                )
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(24.))
            .child(img(crate::assets::app_icon()).size(px(112.)).rounded(px(25.)))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(6.))
                    .child(div().text_size(px(17.)).font_weight(gpui::FontWeight::SEMIBOLD).child(self.name()))
                    .child(div().text_color(colors.secondary).child("Select a file in the sidebar to start editing.")),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .text_size(px(13.))
                    .child(shortcut("Quick Open", "Ctrl P"))
                    .child(shortcut("Open Another Project", "Ctrl O"))
                    .child(shortcut("Toggle Sidebar", "Ctrl B")),
            )
    }
}
