//! `gitGraph`: 브랜치를 세로 레인으로 두고 커밋을 위에서 아래로 쌓는다.

use super::parse::{clean_lines, label};
use crate::doc::Line;
use crate::render::canvas::{Canvas, truncate, width_of};
use crate::theme::Theme;

enum Ev {
    Commit { lane: usize, text: String, tag: String },
    Branch { from: usize, lane: usize },
    Merge { from: usize, lane: usize, text: String, tag: String },
    Checkout,
}

pub fn render(src: &str, theme: &Theme, width: usize) -> Vec<Line> {
    let mut lanes: Vec<String> = Vec::new();
    let mut cur = 0usize;
    let mut events: Vec<Ev> = Vec::new();

    let lane_of = |lanes: &mut Vec<String>, name: &str| -> usize {
        match lanes.iter().position(|l| l == name) {
            Some(i) => i,
            None => {
                lanes.push(name.to_string());
                lanes.len() - 1
            }
        }
    };
    lane_of(&mut lanes, "main");

    for raw in clean_lines(src) {
        let t = raw.trim();
        let lower = t.to_ascii_lowercase();
        if lower.starts_with("gitgraph") || lower.starts_with("acc") || lower.starts_with("%%") {
            continue;
        }
        if let Some(rest) = strip(&lower, t, "branch") {
            let name = rest.split_whitespace().next().unwrap_or("").to_string();
            let lane = lane_of(&mut lanes, &name);
            events.push(Ev::Branch { from: cur, lane });
            cur = lane;
            continue;
        }
        if let Some(rest) = strip(&lower, t, "checkout").or_else(|| strip(&lower, t, "switch")) {
            let name = rest.split_whitespace().next().unwrap_or("");
            cur = lane_of(&mut lanes, name);
            events.push(Ev::Checkout);
            continue;
        }
        if let Some(rest) = strip(&lower, t, "merge") {
            let name = rest.split_whitespace().next().unwrap_or("");
            let from = lane_of(&mut lanes, name);
            events.push(Ev::Merge { from, lane: cur, text: format!("merge {name}"), tag: field(t, "tag") });
            continue;
        }
        if lower == "commit" || lower.starts_with("commit ") {
            let id = field(t, "id");
            let ty = field(t, "type");
            let text = if id.is_empty() { String::new() } else { id };
            events.push(Ev::Commit { lane: cur, text, tag: if ty == "REVERSE" { String::from("revert") } else { field(t, "tag") } });
            continue;
        }
    }
    if events.is_empty() {
        return Vec::new();
    }

    // 레인 이름이 겹치지 않을 만큼 간격을 둔다.
    let name_w = lanes.iter().map(|l| width_of(l)).max().unwrap_or(4);
    let room = (width.saturating_sub(24) / lanes.len().max(1)).max(4);
    let gap = (name_w + 2).clamp(4, room.max(4));
    let lane_x = |i: usize| 2 + i * gap;
    let head_w = lane_x(lanes.len().saturating_sub(1)) + 4;
    let rows = events.iter().filter(|e| !matches!(e, Ev::Checkout)).count();
    let mut c = Canvas::new(width.max(head_w + 10), rows * 2 + 3);
    let line_st = theme.diagram_edge;
    let dot_st = theme.diagram_border;

    // 레인 이름
    for (i, name) in lanes.iter().enumerate() {
        c.text(lane_x(i).saturating_sub(1), 0, &truncate(name, gap + 1), theme.diagram_title);
    }

    let mut y = 2usize;
    let mut alive: Vec<bool> = vec![false; lanes.len()];
    alive[0] = true;
    let last = events.iter().rposition(|e| !matches!(e, Ev::Checkout)).unwrap_or(0);
    for (ei, ev) in events.iter().enumerate() {
        match ev {
            Ev::Checkout => continue,
            Ev::Branch { from, lane } => {
                alive[*lane] = true;
                // 부모 레인에서 갈라져 나오는 선
                c.hline(lane_x(*from) + 1, lane_x(*lane) - 1, y, '─', line_st);
                c.draw(lane_x(*from), y, '├', line_st);
                c.draw(lane_x(*lane), y, '┐', line_st);
                c.text(lane_x(lanes.len() - 1) + 4, y, &format!("branch {}", lanes[*lane]), theme.diagram_note);
            }
            Ev::Commit { lane, text, tag } => {
                c.set(lane_x(*lane), y, '●', dot_st);
                let mut s = text.clone();
                if !tag.is_empty() {
                    s.push_str(&format!("  ⌂{tag}"));
                }
                c.text(lane_x(lanes.len() - 1) + 4, y, s.trim(), theme.diagram_label);
            }
            Ev::Merge { from, lane, text, tag } => {
                c.hline(lane_x(*from.min(lane)) + 1, lane_x(*from.max(lane)) - 1, y, '─', line_st);
                c.draw(lane_x(*from), y, if from > lane { '┘' } else { '└' }, line_st);
                c.set(lane_x(*lane), y, '◆', dot_st);
                alive[*from] = false;
                let mut s = text.clone();
                if !tag.is_empty() {
                    s.push_str(&format!("  ⌂{tag}"));
                }
                c.text(lane_x(lanes.len() - 1) + 4, y, &s, theme.diagram_note);
            }
        }
        // 살아 있는 레인을 다음 줄까지 잇는다(마지막 줄 뒤로는 긋지 않는다).
        for (i, on) in alive.iter().enumerate() {
            if *on {
                c.draw_soft(lane_x(i), y, '│', line_st);
                if ei < last {
                    c.draw_soft(lane_x(i), y + 1, '│', line_st);
                }
            }
        }
        y += 2;
    }
    c.grow_to(y);
    c.into_lines()
}

/// `commit id: "x"` 같은 줄에서 항목 값을 읽는다.
fn field(t: &str, key: &str) -> String {
    let pat = format!("{key}:");
    let Some(p) = t.find(&pat) else { return String::new() };
    let rest = t[p + pat.len()..].trim_start();
    if let Some(q) = rest.strip_prefix('"') {
        return label(q.split('"').next().unwrap_or(""));
    }
    label(rest.split_whitespace().next().unwrap_or(""))
}

fn strip(lower: &str, orig: &str, kw: &str) -> Option<String> {
    let rest = lower.strip_prefix(kw)?;
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    Some(orig[kw.len()..].trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out(src: &str) -> Vec<String> {
        render(src, &Theme::notty(), 80).iter().map(Line::plain).collect()
    }

    #[test]
    fn commits_and_branches() {
        let o = out("gitGraph\n commit id: \"init\"\n branch develop\n commit id: \"base\"\n");
        assert!(o[0].contains("main") && o[0].contains("develop"));
        assert!(o.iter().any(|l| l.contains("init")));
        assert!(o.iter().any(|l| l.contains("branch develop")));
        assert!(o.iter().any(|l| l.contains('●')));
    }

    #[test]
    fn merge_marks_diamond_and_tag() {
        let o = out("gitGraph\n commit\n branch dev\n commit\n checkout main\n merge dev tag: \"v1\"\n");
        assert!(o.iter().any(|l| l.contains('◆')));
        assert!(o.iter().any(|l| l.contains("⌂v1")));
    }

    #[test]
    fn field_reads_quoted_value() {
        assert_eq!(field(r#"commit id: "기간 포맷""#, "id"), "기간 포맷");
        assert_eq!(field("commit", "id"), "");
    }
}
