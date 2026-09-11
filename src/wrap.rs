//! 스타일 span을 유지하면서 표시 폭 기준으로 줄을 바꾼다.

use crate::doc::{Line, Span, Style};
use unicode_width::UnicodeWidthChar;

/// 줄바꿈 단위: 단어 하나 또는 공백 하나.
#[derive(Debug)]
struct Token {
    text: String,
    style: Style,
    width: usize,
    is_space: bool,
}

fn tokenize(line: &Line) -> Vec<Token> {
    let mut out = Vec::new();
    for span in &line.spans {
        let mut cur = String::new();
        let mut cur_w = 0;
        let mut cur_space: Option<bool> = None;
        for ch in span.text.chars() {
            let w = UnicodeWidthChar::width(ch).unwrap_or(0);
            let sp = ch == ' ';
            // 한자/가나처럼 띄어쓰기가 없는 문자는 글자 단위로 줄바꿈한다.
            // 한글은 어절(공백) 단위로 유지한다.
            let breakable = sp || is_ideographic(ch);
            if !cur.is_empty() && (cur_space != Some(sp) || breakable) {
                out.push(Token { text: std::mem::take(&mut cur), style: span.style, width: cur_w, is_space: cur_space.unwrap_or(false) });
                cur_w = 0;
            }
            cur.push(ch);
            cur_w += w;
            cur_space = Some(sp);
            if breakable {
                out.push(Token { text: std::mem::take(&mut cur), style: span.style, width: cur_w, is_space: sp });
                cur_w = 0;
                cur_space = None;
            }
        }
        if !cur.is_empty() {
            out.push(Token { text: cur, style: span.style, width: cur_w, is_space: cur_space.unwrap_or(false) });
        }
    }
    out
}

/// 띄어쓰기 없이 이어지는 문자(한자, 가나, 전각 기호)인지.
fn is_ideographic(ch: char) -> bool {
    matches!(ch as u32,
        0x2E80..=0x2FDF   // CJK 부수
        | 0x3000..=0x303F // CJK 기호/구두점
        | 0x3040..=0x30FF // 히라가나, 가타카나
        | 0x3400..=0x4DBF // CJK 확장 A
        | 0x4E00..=0x9FFF // CJK 통합 한자
        | 0xF900..=0xFAFF // CJK 호환 한자
        | 0xFF00..=0xFF60 // 전각 영숫자/기호
        | 0x20000..=0x3134F // CJK 확장 B 이후
    )
}

/// 폭이 `width`보다 큰 단일 토큰을 글자 단위로 쪼갠다.
fn split_hard(tok: &Token, width: usize) -> Vec<Token> {
    let mut pieces = Vec::new();
    let mut cur = String::new();
    let mut cur_w = 0;
    for ch in tok.text.chars() {
        let w = UnicodeWidthChar::width(ch).unwrap_or(0);
        if cur_w + w > width && !cur.is_empty() {
            pieces.push(Token { text: std::mem::take(&mut cur), style: tok.style, width: cur_w, is_space: false });
            cur_w = 0;
        }
        cur.push(ch);
        cur_w += w;
    }
    if !cur.is_empty() {
        pieces.push(Token { text: cur, style: tok.style, width: cur_w, is_space: false });
    }
    pieces
}

/// 한 줄을 `width` 폭 안에 들어가도록 여러 줄로 나눈다. 줄 끝 공백은 제거된다.
pub fn wrap_line(line: &Line, width: usize) -> Vec<Line> {
    let width = width.max(1);
    let tokens = tokenize(line);
    let mut lines: Vec<Line> = Vec::new();
    let mut cur = Line::new();
    let mut cur_w = 0usize;

    let flush = |cur: &mut Line, lines: &mut Vec<Line>| {
        // 줄 끝 공백 제거
        while let Some(last) = cur.spans.last_mut() {
            let trimmed = last.text.trim_end_matches(' ').len();
            last.text.truncate(trimmed);
            if last.text.is_empty() {
                cur.spans.pop();
            } else {
                break;
            }
        }
        lines.push(std::mem::take(cur));
    };

    for tok in tokens {
        if tok.is_space {
            if cur_w == 0 {
                continue; // 줄 머리 공백 생략
            }
            if cur_w + tok.width > width {
                flush(&mut cur, &mut lines);
                cur_w = 0;
                continue;
            }
            cur_w += tok.width;
            push_merge(&mut cur, tok.text, tok.style);
            continue;
        }
        if tok.width > width {
            for piece in split_hard(&tok, width) {
                if cur_w + piece.width > width && cur_w > 0 {
                    flush(&mut cur, &mut lines);
                    cur_w = 0;
                }
                cur_w += piece.width;
                push_merge(&mut cur, piece.text, piece.style);
            }
            continue;
        }
        if cur_w + tok.width > width && cur_w > 0 {
            flush(&mut cur, &mut lines);
            cur_w = 0;
        }
        cur_w += tok.width;
        push_merge(&mut cur, tok.text, tok.style);
    }
    if !cur.spans.is_empty() || lines.is_empty() {
        flush(&mut cur, &mut lines);
    }
    lines
}

/// 같은 스타일의 연속 텍스트는 하나의 span으로 합친다.
fn push_merge(line: &mut Line, text: String, style: Style) {
    if let Some(last) = line.spans.last_mut()
        && last.style == style {
            last.text.push_str(&text);
            return;
        }
    line.push(Span::new(text, style));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Color;

    fn plain(lines: &[Line]) -> Vec<String> {
        lines.iter().map(Line::plain).collect()
    }

    #[test]
    fn wraps_on_spaces() {
        let l = Line::from_spans(vec![Span::raw("the quick brown fox jumps")]);
        assert_eq!(plain(&wrap_line(&l, 10)), vec!["the quick", "brown fox", "jumps"]);
    }

    #[test]
    fn wraps_korean_by_character_width() {
        let l = Line::from_spans(vec![Span::raw("안녕하세요 세계")]);
        // 폭 6: "안녕하"(6) / "세요"(4) / "세계"
        assert_eq!(plain(&wrap_line(&l, 6)), vec!["안녕하", "세요", "세계"]);
    }

    #[test]
    fn korean_wraps_at_spaces_when_possible() {
        let l = Line::from_spans(vec![Span::raw("안녕하세요 세계")]);
        assert_eq!(plain(&wrap_line(&l, 10)), vec!["안녕하세요", "세계"]);
    }

    #[test]
    fn japanese_wraps_per_character() {
        let l = Line::from_spans(vec![Span::raw("こんにちは世界")]);
        assert_eq!(plain(&wrap_line(&l, 6)), vec!["こんに", "ちは世", "界"]);
    }

    #[test]
    fn splits_long_word() {
        let l = Line::from_spans(vec![Span::raw("abcdefghijkl")]);
        assert_eq!(plain(&wrap_line(&l, 5)), vec!["abcde", "fghij", "kl"]);
    }

    #[test]
    fn keeps_styles_across_wrap() {
        let bold = Style::new().bold();
        let l = Line::from_spans(vec![Span::raw("aa "), Span::new("bb cc", bold), Span::raw(" dd")]);
        let out = wrap_line(&l, 5);
        assert_eq!(plain(&out), vec!["aa bb", "cc dd"]);
        assert_eq!(out[0].spans[1].style, bold);
        assert_eq!(out[1].spans[0].style, bold);
        assert_eq!(out[1].spans[1].style, Style::new());
    }

    #[test]
    fn empty_line_yields_one_empty_line() {
        let out = wrap_line(&Line::new(), 10);
        assert_eq!(out.len(), 1);
        assert!(out[0].is_empty());
    }

    #[test]
    fn merges_same_style_spans() {
        let red = Style::new().fg(Color::Red);
        let l = Line::from_spans(vec![Span::new("a", red), Span::new("b", red)]);
        let out = wrap_line(&l, 10);
        assert_eq!(out[0].spans.len(), 1);
        assert_eq!(out[0].spans[0].text, "ab");
    }
}
