//! 다이어그램을 그리기 위한 문자 격자 캔버스.
//!
//! 박스 문자는 방향 비트마스크로 합성되므로, 선이 교차하면 `┼`·`├` 같은
//! 이음 문자가 자동으로 만들어진다. 전각 문자(한글·한자)는 두 칸을 차지하며
//! 뒤 칸은 연속 표시로 채운다.

use crate::doc::{Line, Span, Style};
use unicode_width::UnicodeWidthChar;

/// 전각 문자의 뒤 칸을 나타내는 표식.
const CONT: char = '\u{0}';

#[derive(Clone, Copy, PartialEq, Eq)]
struct Cell {
    ch: char,
    style: Style,
}

impl Default for Cell {
    fn default() -> Self {
        Cell { ch: ' ', style: Style::new() }
    }
}

pub const UP: u8 = 1;
pub const RIGHT: u8 = 2;
pub const DOWN: u8 = 4;
pub const LEFT: u8 = 8;

/// 박스 문자 ↔ 방향 비트마스크.
const BOX_CHARS: [(char, u8); 11] = [
    ('─', LEFT | RIGHT),
    ('│', UP | DOWN),
    ('┌', RIGHT | DOWN),
    ('┐', LEFT | DOWN),
    ('└', UP | RIGHT),
    ('┘', UP | LEFT),
    ('├', UP | RIGHT | DOWN),
    ('┤', UP | DOWN | LEFT),
    ('┬', LEFT | RIGHT | DOWN),
    ('┴', LEFT | RIGHT | UP),
    ('┼', UP | RIGHT | DOWN | LEFT),
];

fn box_mask(ch: char) -> Option<u8> {
    BOX_CHARS.iter().find(|(c, _)| *c == ch).map(|(_, m)| *m)
}

fn mask_char(mask: u8) -> char {
    BOX_CHARS.iter().find(|(_, m)| *m == mask).map(|(c, _)| *c).unwrap_or('┼')
}

pub struct Canvas {
    w: usize,
    h: usize,
    cells: Vec<Cell>,
}

impl Canvas {
    pub fn new(w: usize, h: usize) -> Self {
        Canvas { w, h, cells: vec![Cell::default(); w * h] }
    }

    /// 아래쪽으로 `rows`줄 더 확보한다.
    pub fn grow_to(&mut self, h: usize) {
        if h > self.h {
            self.cells.resize(self.w * h, Cell::default());
            self.h = h;
        }
    }

    fn idx(&self, x: usize, y: usize) -> Option<usize> {
        if x < self.w && y < self.h { Some(y * self.w + x) } else { None }
    }

    pub fn get(&self, x: usize, y: usize) -> char {
        self.idx(x, y).map(|i| self.cells[i].ch).unwrap_or(' ')
    }

    /// 한 칸을 덮어쓴다. 전각 문자를 덮으면 짝이 되는 칸을 공백으로 되돌린다.
    pub fn set(&mut self, x: usize, y: usize, ch: char, style: Style) {
        let Some(i) = self.idx(x, y) else { return };
        // 덮어쓰는 자리가 전각 문자의 앞/뒤 칸이면 짝을 지운다.
        if self.cells[i].ch == CONT && x > 0 {
            let j = i - 1;
            self.cells[j] = Cell::default();
        }
        if x + 1 < self.w && self.cells[i + 1].ch == CONT && UnicodeWidthChar::width(self.cells[i].ch).unwrap_or(1) == 2 {
            self.cells[i + 1] = Cell::default();
        }
        self.cells[i] = Cell { ch, style };
    }

    /// 박스 문자는 기존 문자와 방향을 합쳐 이음 문자로 만든다.
    pub fn draw(&mut self, x: usize, y: usize, ch: char, style: Style) {
        let old = self.get(x, y);
        match (box_mask(old), box_mask(ch)) {
            (Some(a), Some(b)) if a != b => self.set(x, y, mask_char(a | b), style),
            _ => self.set(x, y, ch, style),
        }
    }

    /// 빈 칸에만 쓴다(이미 그려진 것을 덮지 않는다).
    pub fn draw_soft(&mut self, x: usize, y: usize, ch: char, style: Style) {
        if self.get(x, y) == ' ' {
            self.set(x, y, ch, style);
        }
    }

    /// 왼쪽 위 `(x, y)`부터 문자열을 쓴다. 반환값은 사용한 폭.
    pub fn text(&mut self, x: usize, y: usize, s: &str, style: Style) -> usize {
        let mut cx = x;
        for ch in s.chars() {
            if ch == '\n' {
                break;
            }
            let w = UnicodeWidthChar::width(ch).unwrap_or(0);
            if w == 0 {
                continue;
            }
            if cx >= self.w {
                break;
            }
            self.set(cx, y, ch, style);
            if w == 2 {
                if cx + 1 < self.w {
                    if let Some(i) = self.idx(cx + 1, y) {
                        self.cells[i] = Cell { ch: CONT, style };
                    }
                } else {
                    // 마지막 칸에 반만 들어가면 지운다.
                    self.set(cx, y, ' ', style);
                }
            }
            cx += w;
        }
        cx.saturating_sub(x)
    }

    /// `x0..=x1` 가로선.
    pub fn hline(&mut self, x0: usize, x1: usize, y: usize, ch: char, style: Style) {
        let (a, b) = if x0 <= x1 { (x0, x1) } else { (x1, x0) };
        for x in a..=b {
            self.draw(x, y, ch, style);
        }
    }

    /// `y0..=y1` 세로선.
    pub fn vline(&mut self, x: usize, y0: usize, y1: usize, ch: char, style: Style) {
        let (a, b) = if y0 <= y1 { (y0, y1) } else { (y1, y0) };
        for y in a..=b {
            self.draw(x, y, ch, style);
        }
    }

    /// 모서리가 둥근 사각 테두리.
    pub fn rect(&mut self, x: usize, y: usize, w: usize, h: usize, style: Style, round: bool) {
        if w < 2 || h < 2 {
            return;
        }
        let (x1, y1) = (x + w - 1, y + h - 1);
        self.hline(x + 1, x1 - 1, y, '─', style);
        self.hline(x + 1, x1 - 1, y1, '─', style);
        self.vline(x, y + 1, y1 - 1, '│', style);
        self.vline(x1, y + 1, y1 - 1, '│', style);
        let (tl, tr, bl, br) = if round { ('╭', '╮', '╰', '╯') } else { ('┌', '┐', '└', '┘') };
        self.set(x, y, tl, style);
        self.set(x1, y, tr, style);
        self.set(x, y1, bl, style);
        self.set(x1, y1, br, style);
    }

    /// 표 칸처럼 서로 맞닿는 테두리. 모서리도 기존 문자와 합친다.
    pub fn rect_join(&mut self, x: usize, y: usize, w: usize, h: usize, style: Style) {
        if w < 2 || h < 2 {
            return;
        }
        let (x1, y1) = (x + w - 1, y + h - 1);
        self.hline(x + 1, x1 - 1, y, '─', style);
        self.hline(x + 1, x1 - 1, y1, '─', style);
        self.vline(x, y + 1, y1 - 1, '│', style);
        self.vline(x1, y + 1, y1 - 1, '│', style);
        self.draw(x, y, '┌', style);
        self.draw(x1, y, '┐', style);
        self.draw(x, y1, '└', style);
        self.draw(x1, y1, '┘', style);
    }

    /// 테두리 안쪽을 공백으로 비운다.
    pub fn clear_rect(&mut self, x: usize, y: usize, w: usize, h: usize) {
        for yy in y..(y + h).min(self.h) {
            for xx in x..(x + w).min(self.w) {
                self.set(xx, yy, ' ', Style::new());
            }
        }
    }

    /// 캔버스를 스타일 줄 목록으로 변환한다(줄 끝 공백 제거).
    pub fn into_lines(self) -> Vec<Line> {
        let mut out = Vec::with_capacity(self.h);
        for y in 0..self.h {
            let row = &self.cells[y * self.w..(y + 1) * self.w];
            let end = row.iter().rposition(|c| c.ch != ' ' && c.ch != CONT).map(|i| i + 1).unwrap_or(0);
            let mut line = Line::new();
            let mut buf = String::new();
            let mut cur = Style::new();
            for cell in &row[..end] {
                if cell.ch == CONT {
                    continue;
                }
                if !buf.is_empty() && cell.style != cur {
                    line.push(Span::new(std::mem::take(&mut buf), cur));
                }
                if buf.is_empty() {
                    cur = cell.style;
                }
                buf.push(cell.ch);
            }
            if !buf.is_empty() {
                line.push(Span::new(buf, cur));
            }
            out.push(line);
        }
        out
    }
}

/// 표시 폭.
pub fn width_of(s: &str) -> usize {
    s.chars().map(|c| UnicodeWidthChar::width(c).unwrap_or(0)).sum()
}

/// 표시 폭이 `max`를 넘으면 `…`를 붙여 자른다.
pub fn truncate(s: &str, max: usize) -> String {
    if width_of(s) <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut w = 0;
    for ch in s.chars() {
        let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
        if w + cw > max.saturating_sub(1) {
            break;
        }
        out.push(ch);
        w += cw;
    }
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(c: Canvas) -> Vec<String> {
        c.into_lines().iter().map(Line::plain).collect()
    }

    #[test]
    fn draws_box_with_text() {
        let mut c = Canvas::new(10, 3);
        c.rect(0, 0, 7, 3, Style::new(), false);
        c.text(2, 1, "hi", Style::new());
        assert_eq!(plain(c), vec!["┌─────┐", "│ hi  │", "└─────┘"]);
    }

    #[test]
    fn lines_merge_into_junctions() {
        let mut c = Canvas::new(5, 3);
        c.hline(0, 4, 1, '─', Style::new());
        c.vline(2, 0, 2, '│', Style::new());
        assert_eq!(plain(c), vec!["  │", "──┼──", "  │"]);
    }

    #[test]
    fn wide_chars_take_two_cells() {
        let mut c = Canvas::new(8, 1);
        c.text(0, 0, "한글", Style::new());
        c.text(4, 0, "x", Style::new());
        assert_eq!(plain(c), vec!["한글x"]);
        assert_eq!(c_width("한글x"), 5);
    }

    fn c_width(s: &str) -> usize {
        width_of(s)
    }

    #[test]
    fn truncate_adds_ellipsis() {
        assert_eq!(truncate("abcdef", 4), "abc…");
        assert_eq!(truncate("abc", 4), "abc");
    }

    #[test]
    fn overwriting_wide_char_clears_partner() {
        let mut c = Canvas::new(4, 1);
        c.text(0, 0, "한", Style::new());
        c.set(1, 0, 'x', Style::new());
        assert_eq!(plain(c), vec![" x"]);
    }
}
