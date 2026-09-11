//! syntect를 이용한 코드블록 하이라이트.

use crate::doc::{Color, Line, Span, Style};
use crate::theme::Theme;
use std::sync::OnceLock;
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, ThemeSet};
use syntect::parsing::SyntaxSet;

fn syntax_set() -> &'static SyntaxSet {
    static SS: OnceLock<SyntaxSet> = OnceLock::new();
    SS.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn theme_set() -> &'static ThemeSet {
    static TS: OnceLock<ThemeSet> = OnceLock::new();
    TS.get_or_init(ThemeSet::load_defaults)
}

/// 코드 문자열을 줄 단위의 스타일 span으로 변환한다.
/// 언어를 찾지 못하거나 notty 테마이면 평문 span을 돌려준다.
pub fn highlight(code: &str, lang: &str, theme: &Theme) -> Vec<Line> {
    let lines_raw: Vec<&str> = code.lines().collect();
    let plain = |style: Style| -> Vec<Line> {
        lines_raw
            .iter()
            .map(|l| Line::from_spans(vec![Span::new(expand_tabs(l), style)]))
            .collect()
    };
    if theme.is_notty() {
        return plain(Style::new());
    }
    let ss = syntax_set();
    let syntax = find_syntax(ss, lang, lines_raw.first().copied().unwrap_or(""));
    let Some(syntax) = syntax else {
        return plain(theme.code_block);
    };
    let Some(st) = theme_set().themes.get(theme.syntax_theme) else {
        return plain(theme.code_block);
    };
    let mut hl = HighlightLines::new(syntax, st);
    let mut out = Vec::with_capacity(lines_raw.len());
    for raw in &lines_raw {
        let mut line = Line::new();
        let with_nl = format!("{raw}\n");
        match hl.highlight_line(&with_nl, ss) {
            Ok(ranges) => {
                for (s, text) in ranges {
                    let text = text.trim_end_matches('\n');
                    if text.is_empty() {
                        continue;
                    }
                    let fg = if theme.muted_syntax {
                        muted(s.foreground.r, s.foreground.g, s.foreground.b)
                    } else {
                        Color::Rgb(s.foreground.r, s.foreground.g, s.foreground.b)
                    };
                    let mut style = Style::new().fg(fg);
                    if s.font_style.contains(FontStyle::BOLD) {
                        style = style.bold();
                    }
                    if s.font_style.contains(FontStyle::ITALIC) {
                        style = style.italic();
                    }
                    if s.font_style.contains(FontStyle::UNDERLINE) {
                        style = style.underline();
                    }
                    line.push(Span::new(expand_tabs(text), style));
                }
            }
            Err(_) => line.push(Span::new(expand_tabs(raw), theme.code_block)),
        }
        out.push(line);
    }
    out
}

/// 구문 강조 색을 무채색으로 눌러 쓰되, 원래 색이 또렷했던 토큰만 푸른 쪽으로 살짝 기울인다.
/// 밝기 차이는 그대로 남으므로 주석·문자열·키워드는 여전히 구분된다.
fn muted(r: u8, g: u8, b: u8) -> Color {
    let lum = (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32).round();
    let max = r.max(g).max(b) as f32;
    let min = r.min(g).min(b) as f32;
    let sat = if max > 0.0 { (max - min) / max } else { 0.0 };
    let tilt = sat * 34.0;
    let clamp = |v: f32| v.clamp(0.0, 255.0) as u8;
    Color::Rgb(clamp(lum - tilt * 0.7), clamp(lum - tilt * 0.25), clamp(lum + tilt * 0.5))
}

fn find_syntax<'a>(ss: &'a SyntaxSet, lang: &str, first_line: &str) -> Option<&'a syntect::parsing::SyntaxReference> {
    let lang = lang.trim();
    let lang = lang.split(|c: char| c.is_whitespace() || c == ',' || c == '{').next().unwrap_or("");
    if !lang.is_empty() {
        let lower = lang.to_ascii_lowercase();
        let alias = match lower.as_str() {
            "rs" => "rust",
            "py" => "python",
            "sh" | "shell" | "zsh" | "bash" => "bash",
            "js" => "javascript",
            "ts" => "typescript",
            "yml" => "yaml",
            "md" => "markdown",
            "kt" => "kotlin",
            "cpp" | "c++" | "cc" => "c++",
            "golang" => "go",
            "console" | "terminal" => "bash",
            other => other,
        };
        if let Some(s) = ss.find_syntax_by_token(alias) {
            return Some(s);
        }
        if let Some(s) = ss.find_syntax_by_extension(alias) {
            return Some(s);
        }
    }
    ss.find_syntax_by_first_line(first_line)
}

fn expand_tabs(s: &str) -> String {
    if s.contains('\t') { s.replace('\t', "    ") } else { s.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notty_returns_plain_lines() {
        let out = highlight("fn main() {}\nlet x = 1;", "rust", &Theme::notty());
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].plain(), "fn main() {}");
        assert_eq!(out[0].spans[0].style, Style::new());
    }

    #[test]
    fn dark_highlights_rust_with_colors() {
        let out = highlight("fn main() {}", "rust", &Theme::dark());
        assert_eq!(out[0].plain(), "fn main() {}");
        assert!(out[0].spans.len() > 1, "expected multiple colored spans");
        assert!(matches!(out[0].spans[0].style.fg, Color::Rgb(..)));
    }

    /// 무채색 스킴에서는 강조 색이 회색이거나 살짝 푸른 쪽이어야 한다.
    #[test]
    fn muted_syntax_never_goes_warm() {
        let out = highlight("let s = \"문자열\"; // 주석\nfn f() {}", "rust", &Theme::dark());
        let mut seen = 0;
        for line in &out {
            for span in &line.spans {
                let Color::Rgb(r, _, b) = span.style.fg else { continue };
                assert!(b >= r, "붉은 기가 도는 색이 남았다: {:?}", span.style.fg);
                seen += 1;
            }
        }
        assert!(seen > 0);
    }

    #[test]
    fn muted_keeps_brightness_differences() {
        // 어두운 주석과 밝은 식별자는 여전히 밝기로 구분된다.
        let a = muted(0x65, 0x73, 0x7e);
        let b = muted(0xc0, 0xc5, 0xce);
        let (Color::Rgb(_, ag, _), Color::Rgb(_, bg, _)) = (a, b) else { panic!() };
        assert!(bg > ag + 40);
    }

    #[test]
    fn unknown_language_falls_back_to_plain() {
        let out = highlight("hello world", "no-such-lang-xyz", &Theme::dark());
        assert_eq!(out[0].plain(), "hello world");
    }

    #[test]
    fn tabs_are_expanded() {
        let out = highlight("\tx", "", &Theme::notty());
        assert_eq!(out[0].plain(), "    x");
    }
}
