//! 문서 모델 → ANSI 이스케이프 문자열.

use crate::doc::{Color, Line, Style};
use std::fmt::Write;

fn color_code(c: Color, fg: bool) -> Option<String> {
    let base = if fg { 30 } else { 40 };
    let bright = if fg { 90 } else { 100 };
    Some(match c {
        Color::Reset => return None,
        Color::Black => base.to_string(),
        Color::Red => (base + 1).to_string(),
        Color::Green => (base + 2).to_string(),
        Color::Yellow => (base + 3).to_string(),
        Color::Blue => (base + 4).to_string(),
        Color::Magenta => (base + 5).to_string(),
        Color::Cyan => (base + 6).to_string(),
        Color::White => (base + 7).to_string(),
        Color::BrightBlack => bright.to_string(),
        Color::BrightRed => (bright + 1).to_string(),
        Color::BrightGreen => (bright + 2).to_string(),
        Color::BrightYellow => (bright + 3).to_string(),
        Color::BrightBlue => (bright + 4).to_string(),
        Color::BrightMagenta => (bright + 5).to_string(),
        Color::BrightCyan => (bright + 6).to_string(),
        Color::BrightWhite => (bright + 7).to_string(),
        Color::Indexed(i) => format!("{};5;{}", if fg { 38 } else { 48 }, i),
        Color::Rgb(r, g, b) => format!("{};2;{};{};{}", if fg { 38 } else { 48 }, r, g, b),
    })
}

/// 스타일에 해당하는 SGR 시퀀스. 기본 스타일이면 빈 문자열.
pub fn sgr(style: Style) -> String {
    if style == Style::new() {
        return String::new();
    }
    let mut parts: Vec<String> = Vec::new();
    if style.bold {
        parts.push("1".into());
    }
    if style.dim {
        parts.push("2".into());
    }
    if style.italic {
        parts.push("3".into());
    }
    if style.underline {
        parts.push("4".into());
    }
    if style.strikethrough {
        parts.push("9".into());
    }
    if let Some(c) = color_code(style.fg, true) {
        parts.push(c);
    }
    if let Some(c) = color_code(style.bg, false) {
        parts.push(c);
    }
    format!("\x1b[{}m", parts.join(";"))
}

pub const RESET: &str = "\x1b[0m";

pub fn line_to_string(line: &Line, out: &mut String) {
    for span in &line.spans {
        let code = sgr(span.style);
        if code.is_empty() {
            out.push_str(&span.text);
        } else {
            out.push_str(&code);
            out.push_str(&span.text);
            out.push_str(RESET);
        }
    }
}

pub fn to_string(lines: &[Line]) -> String {
    let mut out = String::new();
    for line in lines {
        line_to_string(line, &mut out);
        let _ = writeln!(out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Span;

    #[test]
    fn default_style_has_no_escapes() {
        let l = Line::from_spans(vec![Span::raw("plain")]);
        assert_eq!(to_string(&[l]), "plain\n");
    }

    #[test]
    fn bold_red_span() {
        let l = Line::from_spans(vec![Span::new("x", Style::new().bold().fg(Color::Red))]);
        assert_eq!(to_string(&[l]), "\x1b[1;31mx\x1b[0m\n");
    }

    #[test]
    fn indexed_and_rgb_colors() {
        assert_eq!(sgr(Style::new().fg(Color::Indexed(39))), "\x1b[38;5;39m");
        assert_eq!(sgr(Style::new().bg(Color::Rgb(1, 2, 3))), "\x1b[48;2;1;2;3m");
    }
}
