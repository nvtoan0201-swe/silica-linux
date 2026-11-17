//! Wrapped text for the previews, laid out at the width it is finally given.
//!
//! GPUI's `StyledText` keeps only the lines from its latest measurement, and answers a
//! min- or max-content query with whatever size it measured last. Taffy measures text at
//! several widths and reuses its own cached sizes, so the lines painted could come from a
//! narrower or wider width than the box they were painted in: text broke apart a letter
//! at a time and ran over the blocks below, or was cut off at the bottom of a table cell.
//! This element measures each width separately, gives intrinsic sizes properly (the longest
//! word, the unwrapped line) and wraps again at the final width before painting.

use std::cell::{Cell, RefCell};
use std::ops::Range;
use std::rc::Rc;

use gpui::{
    App, AvailableSpace, Bounds, CursorStyle, DispatchPhase, Element, ElementId, GlobalElementId, Hitbox,
    HitboxBehavior, InspectorElementId, IntoElement, LayoutId, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, Point, SharedString, Size, TextAlign, TextRun, Window, WrappedLine, px,
};
use smallvec::SmallVec;

type ClickHandler = Rc<dyn Fn(usize, &mut Window, &mut App)>;

/// Styled text whose `links` ranges call `on_click` with the index of the link clicked.
pub struct WrappedText {
    id: ElementId,
    text: SharedString,
    runs: Option<Vec<TextRun>>,
    links: Vec<Range<usize>>,
    on_click: Option<ClickHandler>,
}

impl WrappedText {
    pub fn new(id: impl Into<ElementId>, text: impl Into<SharedString>) -> Self {
        WrappedText { id: id.into(), text: text.into(), runs: None, links: Vec::new(), on_click: None }
    }

    /// The fonts and colors of the text, run by run; without them it takes the inherited style.
    pub fn with_runs(mut self, runs: Vec<TextRun>) -> Self {
        self.runs = Some(runs);
        self
    }

    pub fn on_click(
        mut self,
        links: Vec<Range<usize>>,
        handler: impl Fn(usize, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.links = links;
        self.on_click = Some(Rc::new(handler));
        self
    }
}

pub struct Shaped {
    wrap_width: Option<Pixels>,
    lines: SmallVec<[WrappedLine; 1]>,
    size: Size<Pixels>,
}

#[derive(Default)]
pub struct Layouts {
    font_size: Pixels,
    line_height: Pixels,
    align: TextAlign,
    runs: Vec<TextRun>,
    /// Every width measured this frame.
    shaped: Vec<Shaped>,
    /// The width of the longest word, once asked for.
    min_content: Option<Pixels>,
}

impl Layouts {
    fn shape(&mut self, text: &SharedString, wrap_width: Option<Pixels>, window: &mut Window) -> usize {
        if let Some(found) = self.shaped.iter().position(|s| s.wrap_width == wrap_width) {
            return found;
        }
        let lines = window
            .text_system()
            .shape_text(text.clone(), self.font_size, &self.runs, wrap_width, None)
            .unwrap_or_default();
        let mut size = Size::<Pixels>::default();
        for line in &lines {
            let line_size = line.size(self.line_height);
            size.height += line_size.height;
            size.width = size.width.max(line_size.width).ceil();
        }
        self.shaped.push(Shaped { wrap_width, lines, size });
        self.shaped.len() - 1
    }

    /// The unwrapped layout: its width is the text's max-content width.
    fn unwrapped(&mut self, text: &SharedString, window: &mut Window) -> &Shaped {
        let index = self.shape(text, None, window);
        &self.shaped[index]
    }

    fn min_content(&mut self, text: &SharedString, window: &mut Window) -> Pixels {
        if let Some(width) = self.min_content {
            return width;
        }
        let mut longest = px(0.);
        for line in &self.unwrapped(text, window).lines {
            let layout = &line.unwrapped_layout;
            let mut word_start = None;
            for (at, c) in line.text.char_indices().chain([(line.text.len(), ' ')]) {
                match (c.is_whitespace(), word_start) {
                    (true, Some(start)) => {
                        longest = longest.max(layout.x_for_index(at) - layout.x_for_index(start));
                        word_start = None;
                    }
                    (false, None) => word_start = Some(at),
                    _ => {}
                }
            }
        }
        let width = longest.ceil();
        self.min_content = Some(width);
        width
    }

    /// The lines laid out at `width`: unwrapped when they fit, so a line exactly as wide as
    /// its box doesn't wrap over a rounding error.
    fn at_width(&mut self, text: &SharedString, width: Pixels, window: &mut Window) -> &Shaped {
        let unwrapped = self.unwrapped(text, window).size.width;
        let index = if width + px(0.5) >= unwrapped {
            self.shape(text, None, window)
        } else if let Some(near) =
            self.shaped.iter().position(|s| s.wrap_width.is_some_and(|w| (w - width).abs() < px(0.5)))
        {
            near
        } else {
            self.shape(text, Some(width), window)
        };
        &self.shaped[index]
    }
}

pub struct PaintState {
    hitbox: Option<Hitbox>,
    lines: SmallVec<[WrappedLine; 1]>,
}

impl Element for WrappedText {
    type RequestLayoutState = Rc<RefCell<Layouts>>;
    type PrepaintState = PaintState;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        _cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let layouts = Rc::new(RefCell::new(Layouts {
            font_size,
            line_height: style.line_height.to_pixels(font_size.into(), window.rem_size()),
            align: style.text_align,
            runs: self.runs.clone().unwrap_or_else(|| vec![style.to_run(self.text.len())]),
            ..Layouts::default()
        }));
        let text = self.text.clone();
        let measured = layouts.clone();
        let layout_id = window.request_measured_layout(Default::default(), move |known, available, window, _| {
            let mut layouts = measured.borrow_mut();
            let width = match (known.width, available.width) {
                (Some(width), _) | (None, AvailableSpace::Definite(width)) => Some(width),
                (None, AvailableSpace::MaxContent) => None,
                (None, AvailableSpace::MinContent) => Some(layouts.min_content(&text, window)),
            };
            let size = match width {
                Some(width) => layouts.at_width(&text, width, window).size,
                None => layouts.unwrapped(&text, window).size,
            };
            Size { width: known.width.unwrap_or(size.width), height: known.height.unwrap_or(size.height) }
        });
        (layout_id, layouts)
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layouts: &mut Self::RequestLayoutState,
        window: &mut Window,
        _cx: &mut App,
    ) -> PaintState {
        let lines = layouts.borrow_mut().at_width(&self.text, bounds.size.width, window).lines.clone();
        let hitbox = (!self.links.is_empty()).then(|| window.insert_hitbox(bounds, HitboxBehavior::Normal));
        PaintState { hitbox, lines }
    }

    fn paint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layouts: &mut Self::RequestLayoutState,
        state: &mut PaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let (line_height, align) = {
            let layouts = layouts.borrow();
            (layouts.line_height, layouts.align)
        };
        let mut origin = bounds.origin;
        for line in &state.lines {
            line.paint_background(origin, line_height, align, Some(bounds), window, cx).ok();
            line.paint(origin, line_height, align, Some(bounds), window, cx).ok();
            origin.y += line.size(line_height).height;
        }

        let (Some(hitbox), Some(on_click), Some(global_id)) = (state.hitbox.clone(), self.on_click.clone(), global_id)
        else {
            return;
        };
        let lines = Rc::new(state.lines.clone());
        let links = Rc::new(std::mem::take(&mut self.links));
        let link_at = {
            let (lines, links) = (lines.clone(), links.clone());
            move |position: Point<Pixels>| {
                let index = index_for_position(&lines, bounds, line_height, position)?;
                links.iter().position(|range| range.contains(&index))
            }
        };
        if hitbox.is_hovered(window) && link_at(window.mouse_position()).is_some() {
            window.set_cursor_style(CursorStyle::PointingHand, &hitbox);
        }
        window.with_element_state::<(Rc<Cell<Option<usize>>>, Rc<Cell<Option<usize>>>), _>(global_id, |state, window| {
            let (pressed, hovered) = state.unwrap_or_default();
            let view = window.current_view();
            window.on_mouse_event({
                let (hitbox, link_at, hovered) = (hitbox.clone(), link_at.clone(), hovered.clone());
                move |event: &MouseMoveEvent, phase, window, cx| {
                    let link = if hitbox.is_hovered(window) { link_at(event.position) } else { None };
                    if phase == DispatchPhase::Bubble && hovered.replace(link) != link {
                        cx.notify(view);
                    }
                }
            });
            window.on_mouse_event({
                let (hitbox, link_at, pressed) = (hitbox.clone(), link_at.clone(), pressed.clone());
                move |event: &MouseDownEvent, phase, window, _| {
                    if phase == DispatchPhase::Bubble && hitbox.is_hovered(window) {
                        pressed.set(link_at(event.position));
                    }
                }
            });
            window.on_mouse_event({
                let pressed = pressed.clone();
                move |event: &MouseUpEvent, phase, window, cx| {
                    if phase != DispatchPhase::Bubble {
                        return;
                    }
                    let down = pressed.take();
                    if hitbox.is_hovered(window) && down.is_some() && link_at(event.position) == down {
                        on_click(down.unwrap_or_default(), window, cx);
                    }
                }
            });
            ((), (pressed, hovered))
        });
    }
}

/// The byte index of the character under `position`, if there is one.
fn index_for_position(
    lines: &[WrappedLine],
    bounds: Bounds<Pixels>,
    line_height: Pixels,
    position: Point<Pixels>,
) -> Option<usize> {
    let mut origin = bounds.origin;
    let mut start = 0;
    for line in lines {
        let bottom = origin.y + line.size(line_height).height;
        if position.y < bottom {
            return line.index_for_position(position - origin, line_height).ok().map(|index| start + index);
        }
        origin.y = bottom;
        start += line.len() + 1;
    }
    None
}

impl IntoElement for WrappedText {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}
