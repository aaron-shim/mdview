//! 렌더링 테마(dark / light / notty).

use crate::doc::{Color, Style};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeKind {
    Dark,
    Light,
    NoTty,
}

#[derive(Clone, Debug)]
pub struct Theme {
    pub kind: ThemeKind,
    pub text: Style,
    pub heading: [Style; 6],
    /// 제목 아래에 그을 줄. `None`이면 긋지 않는다.
    pub heading_rule: [Option<char>; 6],
    pub emphasis: Style,
    pub strong: Style,
    pub strikethrough: Style,
    pub code: Style,
    pub code_block: Style,
    pub link: Style,
    pub link_url: Style,
    pub image: Style,
    pub quote: Style,
    pub quote_bar: Style,
    pub list_marker: Style,
    pub task_done: Style,
    pub task_todo: Style,
    pub rule: Style,
    pub table_header: Style,
    pub table_border: Style,
    pub html: Style,
    /// 수식
    pub math: Style,
    /// 다이어그램 테두리
    pub diagram_border: Style,
    /// 다이어그램 노드 글자
    pub diagram_label: Style,
    /// 다이어그램 연결선
    pub diagram_edge: Style,
    /// 다이어그램 제목·강조
    pub diagram_title: Style,
    /// 다이어그램 보조 설명
    pub diagram_note: Style,
    /// syntect 테마 이름
    pub syntax_theme: &'static str,
    /// 구문 강조 색을 무채색+푸른 계열로 눌러서 쓸지
    pub muted_syntax: bool,
    /// 문서 왼쪽 여백
    pub margin: usize,
}

impl Theme {
    pub fn dark() -> Self {
        let s = Style::new;
        Theme {
            kind: ThemeKind::Dark,
            text: s().fg(Color::Indexed(252)),
            heading: [
                s().fg(Color::Indexed(110)).bold(),
                s().fg(Color::Indexed(110)).bold(),
                s().fg(Color::Indexed(109)).bold(),
                s().fg(Color::Indexed(245)).bold(),
                s().fg(Color::Indexed(245)).bold(),
                s().fg(Color::Indexed(245)).bold(),
            ],
            heading_rule: [Some('━'), Some('─'), None, None, None, None],
            emphasis: s().italic(),
            strong: s().bold(),
            strikethrough: s().strikethrough().fg(Color::Indexed(245)),
            code: s().fg(Color::Indexed(110)).bg(Color::Indexed(236)),
            code_block: s().fg(Color::Indexed(245)),
            link: s().fg(Color::Indexed(110)).underline(),
            link_url: s().fg(Color::Indexed(66)),
            image: s().fg(Color::Indexed(109)),
            quote: s().fg(Color::Indexed(245)).italic(),
            quote_bar: s().fg(Color::Indexed(240)),
            list_marker: s().fg(Color::Indexed(67)),
            task_done: s().fg(Color::Indexed(110)),
            task_todo: s().fg(Color::Indexed(244)),
            rule: s().fg(Color::Indexed(240)),
            table_header: s().fg(Color::Indexed(110)).bold(),
            table_border: s().fg(Color::Indexed(240)),
            html: s().fg(Color::Indexed(240)).dim(),
            math: s().fg(Color::Indexed(109)),
            diagram_border: s().fg(Color::Indexed(67)),
            diagram_label: s().fg(Color::Indexed(252)),
            diagram_edge: s().fg(Color::Indexed(243)),
            diagram_title: s().fg(Color::Indexed(110)).bold(),
            diagram_note: s().fg(Color::Indexed(109)),
            syntax_theme: "base16-ocean.dark",
            muted_syntax: true,
            margin: 2,
        }
    }

    pub fn light() -> Self {
        let s = Style::new;
        Theme {
            kind: ThemeKind::Light,
            text: s().fg(Color::Indexed(237)),
            heading: [
                s().fg(Color::Indexed(25)).bold(),
                s().fg(Color::Indexed(25)).bold(),
                s().fg(Color::Indexed(24)).bold(),
                s().fg(Color::Indexed(241)).bold(),
                s().fg(Color::Indexed(241)).bold(),
                s().fg(Color::Indexed(241)).bold(),
            ],
            heading_rule: [Some('━'), Some('─'), None, None, None, None],
            emphasis: s().italic(),
            strong: s().bold(),
            strikethrough: s().strikethrough().fg(Color::Indexed(245)),
            code: s().fg(Color::Indexed(25)).bg(Color::Indexed(254)),
            code_block: s().fg(Color::Indexed(241)),
            link: s().fg(Color::Indexed(25)).underline(),
            link_url: s().fg(Color::Indexed(66)),
            image: s().fg(Color::Indexed(24)),
            quote: s().fg(Color::Indexed(243)).italic(),
            quote_bar: s().fg(Color::Indexed(250)),
            list_marker: s().fg(Color::Indexed(67)),
            task_done: s().fg(Color::Indexed(25)),
            task_todo: s().fg(Color::Indexed(246)),
            rule: s().fg(Color::Indexed(250)),
            table_header: s().fg(Color::Indexed(25)).bold(),
            table_border: s().fg(Color::Indexed(250)),
            html: s().fg(Color::Indexed(247)).dim(),
            math: s().fg(Color::Indexed(24)),
            diagram_border: s().fg(Color::Indexed(67)),
            diagram_label: s().fg(Color::Indexed(237)),
            diagram_edge: s().fg(Color::Indexed(246)),
            diagram_title: s().fg(Color::Indexed(25)).bold(),
            diagram_note: s().fg(Color::Indexed(66)),
            syntax_theme: "base16-ocean.light",
            muted_syntax: true,
            margin: 2,
        }
    }

    /// 색이 전혀 없는 테마(파이프 출력용).
    pub fn notty() -> Self {
        let s = Style::new;
        Theme {
            kind: ThemeKind::NoTty,
            text: s(),
            heading: [s(); 6],
            heading_rule: [Some('━'), Some('─'), None, None, None, None],
            emphasis: s(),
            strong: s(),
            strikethrough: s(),
            code: s(),
            code_block: s(),
            link: s(),
            link_url: s(),
            image: s(),
            quote: s(),
            quote_bar: s(),
            list_marker: s(),
            task_done: s(),
            task_todo: s(),
            rule: s(),
            table_header: s(),
            table_border: s(),
            html: s(),
            math: s(),
            diagram_border: s(),
            diagram_label: s(),
            diagram_edge: s(),
            diagram_title: s(),
            diagram_note: s(),
            syntax_theme: "",
            muted_syntax: false,
            margin: 2,
        }
    }

    pub fn is_notty(&self) -> bool {
        self.kind == ThemeKind::NoTty
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 기본 색은 무채색이고, 강조에만 푸른 계열을 쓴다.
    #[test]
    fn palette_is_grayscale_with_blue_accents() {
        for theme in [Theme::dark(), Theme::light()] {
            assert!(is_gray(theme.text), "본문은 무채색이어야 한다");
            assert!(is_gray(theme.quote));
            assert!(is_gray(theme.rule));
            assert!(is_gray(theme.table_border));
            assert!(is_blue(theme.heading[0]), "제목은 푸른 계열");
            assert!(is_blue(theme.link));
            assert!(is_blue(theme.diagram_border));
            assert!(is_gray(theme.heading[3]), "작은 제목은 무채색으로 낮춘다");
        }
    }

    /// 256색 표에서 232~255는 회색 계단, 그 밖의 무채색은 16~231 중 r=g=b.
    fn is_gray(st: Style) -> bool {
        match st.fg {
            Color::Indexed(i) => i >= 232 || matches!(i, 0 | 7 | 8 | 15 | 16 | 231),
            Color::Reset => true,
            _ => false,
        }
    }

    /// 푸른 계열: 6×6×6 큐브에서 파랑 성분이 가장 큰 색.
    fn is_blue(st: Style) -> bool {
        let Color::Indexed(i) = st.fg else { return false };
        if !(16..232).contains(&i) {
            return false;
        }
        let c = i - 16;
        let (r, g, b) = (c / 36, (c % 36) / 6, c % 6);
        b > r && b >= g
    }
}
