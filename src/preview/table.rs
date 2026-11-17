//! The CSV and TSV preview: the rows as a table that always fits the editor's width.
//! Columns share the width by how much they hold, and long cells wrap rather than
//! pushing the table off to the side.

use gpui::{
    AnyElement, App, Div, FontWeight, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, div, prelude::FluentBuilder, px, relative,
};

use super::text::WrappedText;
use crate::theme::{self, alpha};

/// Rows past this are left out: every row is laid out on each frame.
const MAX_ROWS: usize = 2000;

/// Splits delimited text into rows of cells. CSV follows RFC 4180 quoting, so a quoted
/// cell can hold the delimiter, `""` and line breaks; TSV has no quoting.
pub(super) fn parse(source: &str, delimiter: char, quoted: bool) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut cell = String::new();
    let mut in_quotes = false;
    let mut chars = source.trim_start_matches('\u{feff}').chars().peekable();
    while let Some(c) = chars.next() {
        if in_quotes {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    cell.push('"');
                }
                '"' => in_quotes = false,
                _ => cell.push(c),
            }
            continue;
        }
        match c {
            '"' if quoted && cell.is_empty() => in_quotes = true,
            '\r' if chars.peek() == Some(&'\n') => {}
            '\n' | '\r' => {
                row.push(std::mem::take(&mut cell));
                rows.push(std::mem::take(&mut row));
            }
            c if c == delimiter => row.push(std::mem::take(&mut cell)),
            _ => cell.push(c),
        }
    }
    if !cell.is_empty() || !row.is_empty() {
        row.push(cell);
        rows.push(row);
    }
    // Blank lines, often at the end, aren't rows.
    rows.retain(|row| !(row.len() == 1 && row[0].trim().is_empty()));
    rows
}

/// Commas, unless the first line has more semicolons, as spreadsheets in many locales export.
fn csv_delimiter(source: &str) -> char {
    let first = source.lines().next().unwrap_or("");
    let (mut commas, mut semicolons, mut in_quotes) = (0, 0, false);
    for c in first.chars() {
        match c {
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => commas += 1,
            ';' if !in_quotes => semicolons += 1,
            _ => {}
        }
    }
    if semicolons > commas { ';' } else { ',' }
}

/// How much of the width each column gets, from how long its cells are. The longest cell
/// counts but doesn't dominate, and no column is starved or given everything.
pub(super) fn column_weights<'a>(columns: usize, rows: impl Iterator<Item = &'a [usize]>) -> Vec<f32> {
    let mut longest = vec![0usize; columns];
    let mut total = vec![0usize; columns];
    let mut count = 0usize;
    for lengths in rows.take(500) {
        count += 1;
        for (column, &len) in lengths.iter().enumerate().take(columns) {
            longest[column] = longest[column].max(len);
            total[column] += len;
        }
    }
    let count = count.max(1) as f32;
    (0..columns)
        .map(|column| {
            let mean = total[column] as f32 / count;
            (mean * 0.6 + longest[column] as f32 * 0.4).clamp(3., 40.)
        })
        .collect()
}

/// A cell of a table that shares the row's width with its neighbours by `weight`, and
/// wraps its text rather than growing past its share. A flex column, so its text is held to
/// the cell's width: in a plain div it could come out wider and be cut off at the cell's edge.
pub(super) fn cell(weight: f32) -> Div {
    let mut cell = div().flex().flex_col().flex_basis(px(0.)).min_w(px(0.)).overflow_hidden();
    cell.style().flex_grow = Some(weight);
    cell
}

fn is_number(text: &str) -> bool {
    let text = text.trim().trim_start_matches(['-', '+', '$', '€', '£']).trim_end_matches('%');
    !text.is_empty() && text.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | ',' | '_' | 'e' | 'E'))
        && text.chars().next().is_some_and(|c| c.is_ascii_digit() || c == '.')
}

/// Renders `source`, the text of a CSV file (`tabs` false) or a TSV file.
pub fn render(source: &str, tabs: bool, cx: &App) -> AnyElement {
    let colors = theme::colors(cx);
    let rows = if tabs { parse(source, '\t', false) } else { parse(source, csv_delimiter(source), true) };
    if rows.is_empty() {
        return crate::ui::unavailable("table", "Empty File", "There are no rows to show.", None, cx)
            .into_any_element();
    }
    let columns = rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
    let lengths: Vec<Vec<usize>> =
        rows.iter().map(|row| row.iter().map(|cell| cell.chars().count()).collect()).collect();
    let weights = column_weights(columns, lengths.iter().map(Vec::as_slice));
    // A column whose cells are all numbers reads right-aligned, as in a spreadsheet.
    let numeric: Vec<bool> = (0..columns)
        .map(|column| {
            let mut cells = rows[1..].iter().take(500).filter_map(|row| row.get(column)).filter(|c| !c.trim().is_empty());
            let mut any = false;
            cells.all(|c| {
                any = true;
                is_number(c)
            }) && any
        })
        .collect();
    let gutter = px(8. + 7.5 * rows.len().min(MAX_ROWS + 1).to_string().len() as f32);

    let row = |number: Option<usize>, cells: &[String], header: bool| {
        div()
            .flex()
            .w_full()
            .border_b_1()
            .border_color(colors.separator)
            .when(header, |d| d.bg(alpha(colors.primary, 0.05)).font_weight(FontWeight::SEMIBOLD))
            .when(number.is_some_and(|n| n % 2 == 0), |d| d.bg(alpha(colors.primary, 0.025)))
            .child(
                div()
                    .w(gutter)
                    .flex_none()
                    .pr(px(6.))
                    .py(px(5.))
                    .text_right()
                    .text_size(px(11.))
                    .text_color(colors.tertiary)
                    .children(number.map(|n| n.to_string())),
            )
            .children((0..columns).map(|column| {
                let text = cells.get(column).map(|c| c.trim()).unwrap_or("");
                cell(weights[column])
                    .px(px(10.))
                    .py(px(5.))
                    .border_l_1()
                    .border_color(colors.separator)
                    .when(!header && numeric[column], |d| d.text_right())
                    .child(WrappedText::new(gpui::ElementId::NamedInteger("table-cell".into(), (number.unwrap_or(0) * columns + column) as u64), text.to_string()))
            }))
    };

    let shown = &rows[1..rows.len().min(MAX_ROWS + 1)];
    let hidden = rows.len() - 1 - shown.len();
    div()
        .size_full()
        .flex()
        .flex_col()
        .font_family(SharedString::from(crate::desktop::fonts().ui.clone()))
        .text_size(px(13.))
        .line_height(relative(1.45))
        .text_color(colors.label)
        .child(row(None, &rows[0], true).flex_none())
        .child(
            div()
                .id("table-preview")
                .flex_1()
                .min_h(px(0.))
                .overflow_y_scroll()
                .children(shown.iter().enumerate().map(|(index, cells)| row(Some(index + 1), cells, false)))
                .when(hidden > 0, |d| {
                    d.child(
                        div()
                            .px(px(16.))
                            .py(px(12.))
                            .text_size(px(12.))
                            .text_color(colors.secondary)
                            .child(format!("{hidden} more rows not shown. Show the source to see them all.")),
                    )
                }),
        )
        .into_any_element()
}
