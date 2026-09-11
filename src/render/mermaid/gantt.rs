//! `gantt`: 날짜 축 위에 작업 막대를 그린다.

use super::parse::{clean_lines, label};
use crate::doc::{Line, Span, Style};
use crate::render::canvas::{Canvas, truncate, width_of};
use crate::theme::Theme;

#[derive(Clone, Debug, Default)]
struct Task {
    name: String,
    id: String,
    start: f64,
    end: f64,
    done: bool,
    active: bool,
    crit: bool,
    milestone: bool,
}

pub fn render(src: &str, theme: &Theme, width: usize) -> Vec<Line> {
    let (title, sections) = parse(src);
    if sections.iter().all(|(_, ts)| ts.is_empty()) {
        return Vec::new();
    }
    let all: Vec<&Task> = sections.iter().flat_map(|(_, ts)| ts.iter()).collect();
    let lo = all.iter().map(|t| t.start).fold(f64::INFINITY, f64::min);
    let hi = all.iter().map(|t| t.end).fold(f64::NEG_INFINITY, f64::max);
    let span = (hi - lo).max(1.0);

    let name_w = sections
        .iter()
        .flat_map(|(_, ts)| ts.iter().map(|t| width_of(&t.name) + 2))
        .chain(sections.iter().map(|(s, _)| width_of(s)))
        .max()
        .unwrap_or(10)
        .min(26);
    let chart_w = width.saturating_sub(name_w + 3).clamp(12, 72);
    let col = |d: f64| -> usize { (((d - lo) / span) * (chart_w - 1) as f64).round().max(0.0) as usize };

    let rows: usize = sections.iter().map(|(_, ts)| ts.len() + 1).sum::<usize>() + 3;
    let mut c = Canvas::new(name_w + chart_w + 3, rows);
    let axis_st = theme.diagram_edge;
    let label_st = theme.diagram_label;

    // 날짜 축
    let x0 = name_w + 1;
    c.hline(x0, x0 + chart_w - 1, 1, '─', axis_st);
    let step = (chart_w / 6).max(8);
    let mut x = 0usize;
    while x < chart_w {
        c.set(x0 + x, 1, '┬', axis_st);
        let d = lo + (x as f64 / (chart_w - 1).max(1) as f64) * span;
        c.text(x0 + x, 0, &fmt_date(d), axis_st);
        x += step;
    }

    let mut y = 2usize;
    for (sec, tasks) in &sections {
        if !sec.is_empty() {
            c.text(0, y, &truncate(sec, name_w), theme.diagram_title);
            y += 1;
        }
        for t in tasks {
            c.text(1, y, &truncate(&t.name, name_w.saturating_sub(1)), label_st);
            let (a, b) = (col(t.start), col(t.end));
            let st = bar_style(t, theme);
            if t.milestone {
                c.text(x0 + a, y, "◆", st);
            } else {
                let ch = bar_char(t);
                c.hline(x0 + a, x0 + b.max(a), y, ch, st);
            }
            y += 1;
        }
    }
    c.grow_to(y);
    let mut out = Vec::new();
    if !title.is_empty() {
        out.push(Line::from_spans(vec![Span::new(title, theme.diagram_title)]));
        out.push(Line::new());
    }
    let mut body = c.into_lines();
    while body.last().is_some_and(|l| l.plain().trim().is_empty()) {
        body.pop();
    }
    out.extend(body);
    out.push(Line::new());
    out.push(Line::from_spans(vec![Span::new("▒ 완료   ▓ 진행중   █ 예정   ◆ 마일스톤", theme.diagram_note)]));
    out
}

fn bar_char(t: &Task) -> char {
    if t.done {
        '▒'
    } else if t.active {
        '▓'
    } else {
        '█'
    }
}

fn bar_style(t: &Task, theme: &Theme) -> Style {
    if t.crit {
        theme.diagram_title
    } else if t.active {
        theme.diagram_border
    } else {
        theme.diagram_edge
    }
}

fn parse(src: &str) -> (String, Vec<(String, Vec<Task>)>) {
    let mut title = String::new();
    let mut sections: Vec<(String, Vec<Task>)> = Vec::new();
    let mut cursor = 0f64; // 시작일이 생략된 작업이 이어붙을 자리
    let mut done: Vec<Task> = Vec::new();

    for raw in clean_lines(src) {
        let t = raw.trim();
        let lower = t.to_ascii_lowercase();
        if lower.starts_with("gantt") || lower.starts_with("dateformat") || lower.starts_with("axisformat") || lower.starts_with("excludes") || lower.starts_with("tickinterval") || lower.starts_with("weekday") || lower.starts_with("todaymarker") || lower.starts_with("acc") {
            continue;
        }
        if let Some(rest) = t.strip_prefix("title ") {
            title = label(rest);
            continue;
        }
        if let Some(rest) = t.strip_prefix("section ") {
            sections.push((label(rest), Vec::new()));
            continue;
        }
        let Some((name, spec)) = t.split_once(':') else { continue };
        let mut task = Task { name: label(name), ..Default::default() };
        let mut start: Option<f64> = None;
        let mut dur: Option<f64> = None;
        let mut after: Option<String> = None;
        for tok in spec.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            match tok.to_ascii_lowercase().as_str() {
                "done" => task.done = true,
                "active" => task.active = true,
                "crit" => task.crit = true,
                "milestone" => task.milestone = true,
                _ => {
                    if let Some(id) = tok.strip_prefix("after ") {
                        after = Some(id.trim().to_string());
                    } else if let Some(d) = parse_date(tok) {
                        start = Some(d);
                    } else if let Some(d) = parse_duration(tok) {
                        dur = Some(d);
                    } else if task.id.is_empty() {
                        task.id = tok.to_string();
                    }
                }
            }
        }
        let begin = match (start, &after) {
            (Some(d), _) => d,
            (None, Some(id)) => done.iter().find(|x| &x.id == id).map(|x| x.end).unwrap_or(cursor),
            _ => cursor,
        };
        task.start = begin;
        task.end = begin + dur.unwrap_or(if task.milestone { 0.0 } else { 1.0 });
        cursor = task.end;
        if sections.is_empty() {
            sections.push((String::new(), Vec::new()));
        }
        done.push(task.clone());
        sections.last_mut().unwrap().1.push(task);
    }
    (title, sections)
}

/// `5d`, `2w`, `12h` → 날 수.
fn parse_duration(s: &str) -> Option<f64> {
    let (num, unit) = s.split_at(s.find(|c: char| c.is_alphabetic())?);
    let n: f64 = num.trim().parse().ok()?;
    Some(match unit {
        "d" => n,
        "w" => n * 7.0,
        "h" => n / 24.0,
        "m" => n / 1440.0,
        "s" => n / 86400.0,
        _ => return None,
    })
}

/// `YYYY-MM-DD` → 에폭 기준 날 수.
fn parse_date(s: &str) -> Option<f64> {
    let d = s.split_whitespace().next()?;
    let mut it = d.split('-');
    let y: i64 = it.next()?.parse().ok()?;
    let m: i64 = it.next()?.parse().ok()?;
    let day: i64 = it.next()?.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&day) {
        return None;
    }
    Some(days_from_civil(y, m, day) as f64)
}

/// 그레고리력 → 1970-01-01 기준 일수 (Howard Hinnant 알고리즘).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// 일수 → `MM/DD`.
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn fmt_date(d: f64) -> String {
    let (_, m, day) = civil_from_days(d.floor() as i64);
    format!("{m:02}/{day:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_roundtrip() {
        let d = days_from_civil(2026, 9, 11);
        assert_eq!(civil_from_days(d), (2026, 9, 11));
        assert_eq!(fmt_date(d as f64), "09/11");
    }

    #[test]
    fn durations_convert_to_days() {
        assert_eq!(parse_duration("5d"), Some(5.0));
        assert_eq!(parse_duration("2w"), Some(14.0));
        assert_eq!(parse_duration("nope"), None);
    }

    #[test]
    fn after_chains_to_previous_task() {
        let (_, secs) = parse("gantt\n section S\n A :done, a1, 2026-09-01, 5d\n B :b1, after a1, 3d\n");
        let ts = &secs[0].1;
        assert_eq!(ts[1].start, ts[0].end);
        assert_eq!(ts[1].end - ts[1].start, 3.0);
        assert!(ts[0].done);
    }

    #[test]
    fn renders_axis_and_bars() {
        let o: Vec<String> = render("gantt\n title 일정\n section 설계\n 요구사항 :done, r1, 2026-09-01, 5d\n", &Theme::notty(), 80)
            .iter()
            .map(Line::plain)
            .collect();
        assert_eq!(o[0], "일정");
        assert!(o.iter().any(|l| l.contains("09/01")));
        assert!(o.iter().any(|l| l.contains('▒')));
        assert!(o.iter().any(|l| l.contains("설계")));
    }

    #[test]
    fn milestone_is_a_diamond() {
        let o: Vec<String> = render("gantt\n section S\n 배포 :milestone, m1, 2026-10-05, 0d\n A :a1, 2026-09-01, 5d\n", &Theme::notty(), 80)
            .iter()
            .map(Line::plain)
            .collect();
        assert!(o.iter().any(|l| l.contains('◆')));
    }
}
