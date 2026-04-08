use crate::TerminalSpan;
use slint::Color as SlintColor;
use std::sync::atomic::{AtomicBool, Ordering};
use vt100::{Color, Parser};

static TERMINAL_DARK_MODE: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Default)]
pub struct TerminalCursorState {
    pub row: u16,
    pub col: u16,
    pub visible: bool,
}

#[derive(Clone, PartialEq)]
pub struct RenderedTerminalSpan {
    pub start_col: i32,
    pub text: String,
    pub cells: i32,
    pub foreground: SlintColor,
    pub background: SlintColor,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

#[derive(Clone, Default, PartialEq)]
pub struct RenderedTerminalLine {
    pub spans: Vec<RenderedTerminalSpan>,
}

pub struct TerminalRenderState {
    parser: Parser,
    rows: u16,
    cols: u16,
    scrollback: usize,
}

impl TerminalRenderState {
    pub fn new(rows: u16, cols: u16, scrollback: usize) -> Self {
        Self {
            parser: Parser::new(rows, cols, scrollback),
            rows,
            cols,
            scrollback,
        }
    }

    pub fn process_with_replies(&mut self, bytes: &[u8]) -> Vec<Vec<u8>> {
        let mut replies = Vec::new();
        let mut segment_start = 0usize;
        let mut index = 0usize;

        while index < bytes.len() {
            if bytes[index] == 0x1b && index + 2 < bytes.len() && bytes[index + 1] == b'[' {
                let mut end = index + 2;
                while end < bytes.len() && !(0x40..=0x7e).contains(&bytes[end]) {
                    end += 1;
                }
                if end >= bytes.len() {
                    break;
                }

                let final_byte = bytes[end];
                let params = std::str::from_utf8(&bytes[index + 2..end]).unwrap_or_default();
                let reply = match (params, final_byte) {
                    ("6", b'n') => Some(ReplyKind::CursorPosition),
                    ("", b'c') | ("0", b'c') => Some(ReplyKind::DeviceAttributes),
                    _ => None,
                };

                if let Some(reply_kind) = reply {
                    self.parser.process(&bytes[segment_start..end + 1]);
                    replies.push(match reply_kind {
                        ReplyKind::CursorPosition => self.cursor_position_report(),
                        ReplyKind::DeviceAttributes => self.device_attributes_report(),
                    });
                    segment_start = end + 1;
                }

                index = end + 1;
                continue;
            }
            index += 1;
        }

        if segment_start < bytes.len() {
            self.parser.process(&bytes[segment_start..]);
        }

        replies
    }

    pub fn resize(&mut self, rows: u16, cols: u16) {
        if self.rows == rows && self.cols == cols {
            return;
        }
        self.rows = rows;
        self.cols = cols;
        self.parser.screen_mut().set_size(rows, cols);
    }

    pub fn clear_buffer(&mut self) {
        self.parser = Parser::new(self.rows, self.cols, self.scrollback);
    }

    pub fn contains_clear_scrollback_escape(bytes: &[u8]) -> bool {
        bytes.windows(4).any(|window| window == b"\x1b[3J")
    }

    pub fn rendered_text(&self) -> String {
        let screen = self.parser.screen();
        let (rows, cols) = screen.size();
        let mut rendered_rows = Vec::with_capacity(rows as usize);
        for row in 0..rows {
            let mut line = String::new();
            for col in 0..cols {
                if let Some(cell) = screen.cell(row, col) {
                    if cell.is_wide_continuation() {
                        continue;
                    }
                    let contents = cell.contents();
                    if contents.is_empty() {
                        line.push(' ');
                    } else {
                        line.push_str(contents);
                    }
                }
            }
            rendered_rows.push(line.trim_end_matches(' ').to_string());
        }
        rendered_rows.join("\n")
    }

    pub fn scrollback(&self) -> usize {
        self.parser.screen().scrollback()
    }

    pub fn scrollback_capacity(&self) -> usize {
        self.scrollback
    }

    pub fn max_scrollback(&mut self) -> usize {
        let screen = self.parser.screen_mut();
        let current = screen.scrollback();
        screen.set_scrollback(usize::MAX);
        let max = screen.scrollback();
        screen.set_scrollback(current);
        max
    }

    pub fn set_scrollback(&mut self, rows: usize) {
        self.parser.screen_mut().set_scrollback(rows);
    }

    pub fn rendered_lines_into(&self, rendered_lines: &mut Vec<RenderedTerminalLine>) {
        let screen = self.parser.screen();
        let (rows, cols) = screen.size();

        rendered_lines.resize_with(rows as usize, RenderedTerminalLine::default);

        for row in 0..rows {
            let spans = &mut rendered_lines[row as usize].spans;
            spans.clear();
            let last_significant_col = last_significant_col(screen, row, cols);
            let mut current_text = String::new();
            let mut current_start_col = 0i32;
            let mut current_cells = 0i32;
            let mut current_fg = default_foreground();
            let mut current_bg = transparent_background();
            let mut current_bold = false;
            let mut current_italic = false;
            let mut current_underline = false;
            let mut has_current = false;

            for col in 0..last_significant_col {
                let Some(cell) = screen.cell(row, col) else {
                    continue;
                };
                if cell.is_wide_continuation() {
                    continue;
                }
                let cell_width = if cell.is_wide() { 2 } else { 1 };

                let mut text = cell.contents().to_string();
                if text.is_empty() {
                    text.push(' ');
                }

                let mut fg = map_terminal_color(cell.fgcolor(), true);
                let mut bg = map_terminal_color(cell.bgcolor(), false);
                let bold = cell.bold();
                let italic = cell.italic();
                let underline = cell.underline();

                if cell.inverse() {
                    std::mem::swap(&mut fg, &mut bg);
                }

                if !has_current {
                    current_start_col = col as i32;
                    current_fg = fg;
                    current_bg = bg;
                    current_bold = bold;
                    current_italic = italic;
                    current_underline = underline;
                    current_text.push_str(&text);
                    current_cells = cell_width;
                    has_current = true;
                    continue;
                }

                if fg == current_fg
                    && bg == current_bg
                    && bold == current_bold
                    && italic == current_italic
                    && underline == current_underline
                {
                    current_text.push_str(&text);
                    current_cells += cell_width;
                } else {
                    spans.push(RenderedTerminalSpan {
                        start_col: current_start_col,
                        text: std::mem::take(&mut current_text),
                        cells: current_cells,
                        foreground: current_fg,
                        background: current_bg,
                        bold: current_bold,
                        italic: current_italic,
                        underline: current_underline,
                    });
                    current_text.push_str(&text);
                    current_start_col = col as i32;
                    current_cells = cell_width;
                    current_fg = fg;
                    current_bg = bg;
                    current_bold = bold;
                    current_italic = italic;
                    current_underline = underline;
                }
            }

            if has_current {
                spans.push(RenderedTerminalSpan {
                    start_col: current_start_col,
                    text: current_text,
                    cells: current_cells,
                    foreground: current_fg,
                    background: current_bg,
                    bold: current_bold,
                    italic: current_italic,
                    underline: current_underline,
                });
            }
        }
        rendered_lines.truncate(rows as usize);
    }
    pub fn cursor_state(&self) -> TerminalCursorState {
        let screen = self.parser.screen();
        let (row, col) = screen.cursor_position();
        TerminalCursorState {
            row,
            col,
            visible: !screen.hide_cursor() && screen.scrollback() == 0,
        }
    }

    fn cursor_position_report(&self) -> Vec<u8> {
        let screen = self.parser.screen();
        let (row, col) = screen.cursor_position();
        format!("\x1b[{};{}R", row + 1, col + 1).into_bytes()
    }

    fn device_attributes_report(&self) -> Vec<u8> {
        b"\x1b[?1;2c".to_vec()
    }
}

fn last_significant_col(screen: &vt100::Screen, row: u16, cols: u16) -> u16 {
    let mut last = 0u16;
    for col in 0..cols {
        let Some(cell) = screen.cell(row, col) else {
            continue;
        };
        if cell.is_wide_continuation() {
            continue;
        }
        if is_significant_cell(cell) {
            let width = if cell.is_wide() { 2 } else { 1 };
            last = col.saturating_add(width as u16);
        }
    }
    last
}

fn is_significant_cell(cell: &vt100::Cell) -> bool {
    let contents = cell.contents();
    if !contents.is_empty() && contents != " " {
        return true;
    }

    cell.bold()
        || cell.italic()
        || cell.underline()
        || cell.inverse()
        || !matches!(cell.fgcolor(), Color::Default)
        || !matches!(cell.bgcolor(), Color::Default)
}

enum ReplyKind {
    CursorPosition,
    DeviceAttributes,
}

pub fn to_ui_spans(spans: &[RenderedTerminalSpan]) -> Vec<TerminalSpan> {
    spans
        .iter()
        .map(|span| TerminalSpan {
            start_col: span.start_col,
            text: span.text.clone().into(),
            cells: span.cells,
            foreground: span.foreground,
            background: span.background,
            bold: span.bold,
            italic: span.italic,
            underline: span.underline,
        })
        .collect()
}

pub fn set_dark_mode(is_dark: bool) {
    TERMINAL_DARK_MODE.store(is_dark, Ordering::Relaxed);
}

fn is_dark_mode() -> bool {
    TERMINAL_DARK_MODE.load(Ordering::Relaxed)
}

fn default_foreground() -> SlintColor {
    if is_dark_mode() {
        SlintColor::from_rgb_u8(0xe6, 0xea, 0xef)
    } else {
        SlintColor::from_rgb_u8(0x3c, 0x40, 0x43)
    }
}

fn transparent_background() -> SlintColor {
    SlintColor::from_argb_u8(0x00, 0x00, 0x00, 0x00)
}

fn map_terminal_color(color: Color, foreground: bool) -> SlintColor {
    match color {
        Color::Default => {
            if foreground {
                default_foreground()
            } else {
                transparent_background()
            }
        }
        Color::Rgb(r, g, b) => SlintColor::from_rgb_u8(r, g, b),
        Color::Idx(index) => map_ansi_index(index),
    }
}

fn map_ansi_index(index: u8) -> SlintColor {
    if is_dark_mode() {
        return match index {
            0 => SlintColor::from_rgb_u8(0x7b, 0x84, 0x8f),
            8 => SlintColor::from_rgb_u8(0x9a, 0xa3, 0xae),
            _ => map_ansi_index_light(index),
        };
    }
    map_ansi_index_light(index)
}

fn map_ansi_index_light(index: u8) -> SlintColor {
    match index {
        0 => SlintColor::from_rgb_u8(0x00, 0x00, 0x00),
        1 => SlintColor::from_rgb_u8(0xcd, 0x31, 0x31),
        2 => SlintColor::from_rgb_u8(0x0d, 0xbc, 0x79),
        3 => SlintColor::from_rgb_u8(0xe5, 0xe5, 0x10),
        4 => SlintColor::from_rgb_u8(0x24, 0x72, 0xc8),
        5 => SlintColor::from_rgb_u8(0xbc, 0x3f, 0xbc),
        6 => SlintColor::from_rgb_u8(0x11, 0xa8, 0xcd),
        7 => SlintColor::from_rgb_u8(0xe5, 0xe5, 0xe5),
        8 => SlintColor::from_rgb_u8(0x66, 0x66, 0x66),
        9 => SlintColor::from_rgb_u8(0xf1, 0x4c, 0x4c),
        10 => SlintColor::from_rgb_u8(0x23, 0xd1, 0x8b),
        11 => SlintColor::from_rgb_u8(0xf5, 0xf5, 0x43),
        12 => SlintColor::from_rgb_u8(0x3b, 0x8e, 0xea),
        13 => SlintColor::from_rgb_u8(0xd6, 0x70, 0xd6),
        14 => SlintColor::from_rgb_u8(0x29, 0xb8, 0xdb),
        15 => SlintColor::from_rgb_u8(0xff, 0xff, 0xff),
        16..=231 => {
            let index = index - 16;
            let r = index / 36;
            let g = (index % 36) / 6;
            let b = index % 6;
            SlintColor::from_rgb_u8(cube_component(r), cube_component(g), cube_component(b))
        }
        232..=255 => {
            let shade = 8 + (index - 232) * 10;
            SlintColor::from_rgb_u8(shade, shade, shade)
        }
    }
}

fn cube_component(value: u8) -> u8 {
    match value {
        0 => 0,
        _ => 55 + value * 40,
    }
}
