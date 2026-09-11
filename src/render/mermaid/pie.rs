//! `pie`: 터미널에서는 원 대신 가로 막대로 비중을 보여준다.

use super::parse::{clean_lines, label};
use crate::doc::{Line, Span};
use crate::render::canvas::{truncate, width_of};
use crate::theme::Theme;

pub fn render(src: &str, theme: &Theme, width: usize) -> Vec<Line> {
    let mut title = String::new();
    let mut show_data = false;
    let mut items: Vec<(String, f64)> = Vec::new();

    for raw in clean_lines(src) {
        let t = raw.trim();
        let lower = t.to_ascii_lowercase();
        if lower.starts_with("pie") {
            show_data = lower.contains("showdata");
            continue;
        }
        if let Some(rest) = t.strip_prefix("title ") {
            title = label(rest);
            continue;
        }
        if lower.starts_with("acc") {
            continue;
        }
        if let Some((name, value)) = t.rsplit_once(':')
            && let Ok(v) = value.trim().parse::<f64>()
        {
            items.push((label(name), v));
        }
    }
    if items.is_empty() {
        return Vec::new();
    }
    let total: f64 = items.iter().map(|(_, v)| *v).sum();
    let name_w = items.iter().map(|(n, _)| width_of(n)).max().unwrap_or(0).min(24);
    let value_w = if show_data {
        items.iter().map(|(_, v)| width_of(&fmt(*v))).max().unwrap_or(0)
    } else {
        0
    };
    // 이름 + 막대 + 값 + 백분율
    let bar_w = width.saturating_sub(name_w + value_w + 14).clamp(8, 48);

    let mut out = Vec::new();
    if !title.is_empty() {
        out.push(Line::from_spans(vec![Span::new(title, theme.diagram_title)]));
        out.push(Line::new());
    }
    for (name, v) in &items {
        let pct = if total > 0.0 { v / total * 100.0 } else { 0.0 };
        let filled = ((pct / 100.0) * bar_w as f64).round() as usize;
        let mut spans = vec![Span::new(format!("{:<w$}  ", pad(name, name_w), w = 0), theme.diagram_label)];
        spans.push(Span::new("█".repeat(filled.max(1)), theme.diagram_border));
        spans.push(Span::new("·".repeat(bar_w.saturating_sub(filled.max(1))), theme.diagram_edge));
        let mut tail = format!("  {pct:>5.1}%");
        if show_data {
            tail.push_str(&format!("  ({})", fmt(*v)));
        }
        spans.push(Span::new(tail, theme.diagram_note));
        out.push(Line::from_spans(spans));
    }
    out.push(Line::new());
    out.push(Line::from_spans(vec![Span::new(format!("합계 {}", fmt(total)), theme.diagram_note)]));
    out
}

fn pad(s: &str, w: usize) -> String {
    let t = truncate(s, w);
    format!("{}{}", t, " ".repeat(w.saturating_sub(width_of(&t))))
}

fn fmt(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 { format!("{}", v.round() as i64) } else { format!("{v}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out(src: &str) -> Vec<String> {
        render(src, &Theme::notty(), 80).iter().map(Line::plain).collect()
    }

    #[test]
    fn bars_show_percentages() {
        let o = out("pie showData\n title 분포\n \"A\" : 75\n \"B\" : 25\n");
        assert_eq!(o[0], "분포");
        assert!(o[2].contains("75.0%") && o[2].contains("(75)"));
        assert!(o[3].contains("25.0%"));
        assert!(o.last().unwrap().contains("합계 100"));
    }

    #[test]
    fn without_showdata_values_are_hidden() {
        let o = out("pie\n \"A\" : 1\n \"B\" : 1\n");
        assert!(o[0].contains("50.0%"));
        assert!(!o[0].contains('('));
    }
}
