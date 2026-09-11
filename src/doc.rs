//! 렌더러와 출력기 사이에서 공유하는 문서 모델.

use unicode_width::UnicodeWidthStr;

/// 터미널 색. 기본 16색 + 256색 인덱스 + RGB.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Color {
    #[default]
    Reset,
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    BrightBlack,
    BrightRed,
    BrightGreen,
    BrightYellow,
    BrightBlue,
    BrightMagenta,
    BrightCyan,
    BrightWhite,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Style {
    pub fg: Color,
    pub bg: Color,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub dim: bool,
}

impl Style {
    pub const fn new() -> Self {
        Style {
            fg: Color::Reset,
            bg: Color::Reset,
            bold: false,
            italic: false,
            underline: false,
            strikethrough: false,
            dim: false,
        }
    }
    pub const fn fg(mut self, c: Color) -> Self {
        self.fg = c;
        self
    }
    pub const fn bg(mut self, c: Color) -> Self {
        self.bg = c;
        self
    }
    pub const fn bold(mut self) -> Self {
        self.bold = true;
        self
    }
    pub const fn italic(mut self) -> Self {
        self.italic = true;
        self
    }
    pub const fn underline(mut self) -> Self {
        self.underline = true;
        self
    }
    pub const fn strikethrough(mut self) -> Self {
        self.strikethrough = true;
        self
    }
    pub const fn dim(mut self) -> Self {
        self.dim = true;
        self
    }

    /// `other`의 속성을 이 스타일 위에 겹친다. 색은 `Reset`이 아닐 때만 덮어쓴다.
    pub fn merge(self, other: Style) -> Style {
        Style {
            fg: if other.fg == Color::Reset { self.fg } else { other.fg },
            bg: if other.bg == Color::Reset { self.bg } else { other.bg },
            bold: self.bold || other.bold,
            italic: self.italic || other.italic,
            underline: self.underline || other.underline,
            strikethrough: self.strikethrough || other.strikethrough,
            dim: self.dim || other.dim,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub style: Style,
}

impl Span {
    pub fn new(text: impl Into<String>, style: Style) -> Self {
        Span { text: text.into(), style }
    }
    pub fn raw(text: impl Into<String>) -> Self {
        Span { text: text.into(), style: Style::new() }
    }
    pub fn width(&self) -> usize {
        UnicodeWidthStr::width(self.text.as_str())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Line {
    pub spans: Vec<Span>,
}

impl Line {
    pub fn new() -> Self {
        Line { spans: Vec::new() }
    }
    pub fn from_spans(spans: Vec<Span>) -> Self {
        Line { spans }
    }
    pub fn push(&mut self, span: Span) {
        if !span.text.is_empty() {
            self.spans.push(span);
        }
    }
    pub fn width(&self) -> usize {
        self.spans.iter().map(Span::width).sum()
    }
    pub fn is_empty(&self) -> bool {
        self.spans.iter().all(|s| s.text.is_empty())
    }
    /// 스타일을 제거한 평문.
    pub fn plain(&self) -> String {
        self.spans.iter().map(|s| s.text.as_str()).collect()
    }
}

#[cfg(test)]
pub fn text_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn width_counts_cjk_as_double() {
        assert_eq!(text_width("abc"), 3);
        assert_eq!(text_width("한글"), 4);
        assert_eq!(Span::raw("a한").width(), 3);
    }

    #[test]
    fn merge_overlays_attributes() {
        let base = Style::new().fg(Color::Red).bold();
        let over = Style::new().italic().fg(Color::Blue);
        let m = base.merge(over);
        assert_eq!(m.fg, Color::Blue);
        assert!(m.bold && m.italic);
        let keep = base.merge(Style::new().underline());
        assert_eq!(keep.fg, Color::Red);
    }

    #[test]
    fn line_plain_skips_empty_spans() {
        let mut l = Line::new();
        l.push(Span::raw("hello "));
        l.push(Span::raw(""));
        l.push(Span::new("world", Style::new().bold()));
        assert_eq!(l.plain(), "hello world");
        assert_eq!(l.spans.len(), 2);
    }
}
