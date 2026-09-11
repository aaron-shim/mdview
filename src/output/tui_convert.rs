//! 문서 모델 → ratatui 텍스트 타입 변환.

use crate::doc::{Color, Line, Style};
use ratatui::style::{Color as RColor, Modifier, Style as RStyle};
use ratatui::text::{Line as RLine, Span as RSpan};

pub fn color(c: Color) -> Option<RColor> {
    Some(match c {
        Color::Reset => return None,
        Color::Black => RColor::Black,
        Color::Red => RColor::Red,
        Color::Green => RColor::Green,
        Color::Yellow => RColor::Yellow,
        Color::Blue => RColor::Blue,
        Color::Magenta => RColor::Magenta,
        Color::Cyan => RColor::Cyan,
        Color::White => RColor::Gray,
        Color::BrightBlack => RColor::DarkGray,
        Color::BrightRed => RColor::LightRed,
        Color::BrightGreen => RColor::LightGreen,
        Color::BrightYellow => RColor::LightYellow,
        Color::BrightBlue => RColor::LightBlue,
        Color::BrightMagenta => RColor::LightMagenta,
        Color::BrightCyan => RColor::LightCyan,
        Color::BrightWhite => RColor::White,
        Color::Indexed(i) => RColor::Indexed(i),
        Color::Rgb(r, g, b) => RColor::Rgb(r, g, b),
    })
}

pub fn style(s: Style) -> RStyle {
    let mut st = RStyle::default();
    if let Some(c) = color(s.fg) {
        st = st.fg(c);
    }
    if let Some(c) = color(s.bg) {
        st = st.bg(c);
    }
    let mut m = Modifier::empty();
    if s.bold {
        m |= Modifier::BOLD;
    }
    if s.italic {
        m |= Modifier::ITALIC;
    }
    if s.underline {
        m |= Modifier::UNDERLINED;
    }
    if s.strikethrough {
        m |= Modifier::CROSSED_OUT;
    }
    if s.dim {
        m |= Modifier::DIM;
    }
    st.add_modifier(m)
}

pub fn line(l: &Line) -> RLine<'static> {
    RLine::from(l.spans.iter().map(|s| RSpan::styled(s.text.clone(), style(s.style))).collect::<Vec<_>>())
}
