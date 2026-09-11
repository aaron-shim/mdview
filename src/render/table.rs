//! 표 레이아웃: 열 폭 계산, 셀 줄바꿈, 테두리 출력.

use crate::doc::{Line, Span, Style};
use crate::theme::Theme;
use crate::wrap::wrap_line;
use pulldown_cmark::Alignment;

/// 한 칸의 내용. `<br>`로 나뉘어 여러 줄일 수 있다.
pub type Cell = Vec<Line>;

pub struct Table {
    pub alignments: Vec<Alignment>,
    pub header: Vec<Cell>,
    pub rows: Vec<Vec<Cell>>,
}

impl Table {
    pub fn new(alignments: Vec<Alignment>) -> Self {
        Table { alignments, header: Vec::new(), rows: Vec::new() }
    }

    fn ncols(&self) -> usize {
        self.alignments.len().max(self.header.len()).max(self.rows.iter().map(Vec::len).max().unwrap_or(0))
    }

    /// 사용 가능한 폭 안에 들어가도록 열 폭을 정한다.
    fn column_widths(&self, avail: usize) -> Vec<usize> {
        let n = self.ncols();
        if n == 0 {
            return Vec::new();
        }
        let mut widths = vec![1usize; n];
        let mut consider = |row: &Vec<Cell>| {
            for (i, cell) in row.iter().enumerate() {
                widths[i] = widths[i].max(cell.iter().map(Line::width).max().unwrap_or(0));
            }
        };
        consider(&self.header);
        for r in &self.rows {
            consider(r);
        }
        // 테두리: "│ " + cells joined by " │ " + " │"  => 3*n + 1
        let overhead = 3 * n + 1;
        let budget = avail.saturating_sub(overhead).max(n);
        while widths.iter().sum::<usize>() > budget {
            // 가장 넓은 열을 한 칸 줄인다.
            let (idx, _) = widths.iter().enumerate().max_by_key(|(_, w)| **w).unwrap();
            if widths[idx] <= 1 {
                break;
            }
            widths[idx] -= 1;
        }
        widths
    }

    pub fn render(&self, theme: &Theme, avail: usize) -> Vec<Line> {
        let widths = self.column_widths(avail);
        if widths.is_empty() {
            return Vec::new();
        }
        let border = theme.table_border;
        let mut out = Vec::new();
        out.push(rule_line(&widths, '┌', '┬', '┐', border));
        if !self.header.is_empty() {
            out.extend(self.render_row(&self.header, &widths, theme, Some(theme.table_header)));
            out.push(rule_line(&widths, '├', '┼', '┤', border));
        }
        for (i, row) in self.rows.iter().enumerate() {
            out.extend(self.render_row(row, &widths, theme, None));
            if i + 1 < self.rows.len() {
                out.push(rule_line(&widths, '├', '┼', '┤', border));
            }
        }
        out.push(rule_line(&widths, '└', '┴', '┘', border));
        out
    }

    fn render_row(&self, row: &[Cell], widths: &[usize], theme: &Theme, cell_style: Option<Style>) -> Vec<Line> {
        let n = widths.len();
        let empty: Cell = Vec::new();
        // 각 셀을 폭에 맞게 줄바꿈
        let wrapped: Vec<Vec<Line>> = (0..n)
            .map(|i| {
                let cell = row.get(i).unwrap_or(&empty);
                cell.iter()
                    .flat_map(|l| {
                        let l = match cell_style {
                            Some(st) => Line::from_spans(l.spans.iter().map(|s| Span::new(s.text.clone(), s.style.merge(st))).collect()),
                            None => l.clone(),
                        };
                        wrap_line(&l, widths[i])
                    })
                    .collect()
            })
            .collect();
        let height = wrapped.iter().map(Vec::len).max().unwrap_or(1).max(1);
        let border = theme.table_border;
        let mut out = Vec::with_capacity(height);
        for r in 0..height {
            let mut line = Line::new();
            line.push(Span::new("│", border));
            for c in 0..n {
                line.push(Span::raw(" "));
                let cell_line = wrapped[c].get(r).cloned().unwrap_or_default();
                let pad = widths[c].saturating_sub(cell_line.width());
                let align = self.alignments.get(c).copied().unwrap_or(Alignment::None);
                let (left, right) = match align {
                    Alignment::Right => (pad, 0),
                    Alignment::Center => (pad / 2, pad - pad / 2),
                    _ => (0, pad),
                };
                line.push(Span::raw(" ".repeat(left)));
                line.spans.extend(cell_line.spans);
                line.push(Span::raw(" ".repeat(right)));
                line.push(Span::raw(" "));
                line.push(Span::new("│", border));
            }
            out.push(line);
        }
        out
    }
}

fn rule_line(widths: &[usize], l: char, m: char, r: char, style: Style) -> Line {
    let mut s = String::new();
    s.push(l);
    for (i, w) in widths.iter().enumerate() {
        s.push_str(&"─".repeat(w + 2));
        s.push(if i + 1 < widths.len() { m } else { r });
    }
    Line::from_spans(vec![Span::new(s, style)])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(s: &str) -> Cell {
        vec![Line::from_spans(vec![Span::raw(s)])]
    }

    #[test]
    fn renders_simple_table() {
        let mut t = Table::new(vec![Alignment::None, Alignment::Right]);
        t.header = vec![cell("name"), cell("n")];
        t.rows = vec![vec![cell("a"), cell("1")], vec![cell("bb"), cell("22")]];
        let out: Vec<String> = t.render(&Theme::notty(), 80).iter().map(Line::plain).collect();
        assert_eq!(
            out,
            vec![
                "┌──────┬────┐",
                "│ name │  n │",
                "├──────┼────┤",
                "│ a    │  1 │",
                "├──────┼────┤",
                "│ bb   │ 22 │",
                "└──────┴────┘",
            ]
        );
    }

    #[test]
    fn wraps_cells_when_too_wide() {
        let mut t = Table::new(vec![Alignment::None]);
        t.header = vec![cell("h")];
        t.rows = vec![vec![cell("one two three four")]];
        let out: Vec<String> = t.render(&Theme::notty(), 14).iter().map(Line::plain).collect();
        // 폭 14 - 4 = 10 열 폭
        assert_eq!(out[3], "│ one two    │");
        assert_eq!(out[4], "│ three four │");
    }

    #[test]
    fn korean_cells_align() {
        let mut t = Table::new(vec![Alignment::None, Alignment::None]);
        t.header = vec![cell("이름"), cell("x")];
        t.rows = vec![vec![cell("가"), cell("y")]];
        let out: Vec<String> = t.render(&Theme::notty(), 80).iter().map(Line::plain).collect();
        assert_eq!(out[1], "│ 이름 │ x │");
        assert_eq!(out[3], "│ 가   │ y │");
    }
}
