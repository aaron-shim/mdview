//! `sequenceDiagram`: 참가자를 세로줄로 세우고 메시지를 가로 화살표로 그린다.

use super::parse::{clean_lines, label};
use crate::doc::Line;
use crate::render::canvas::{Canvas, truncate, width_of};
use crate::theme::Theme;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// 실선 화살표 `->>`
    Solid,
    /// 점선 화살표 `-->>`
    Dashed,
    /// 소멸 `-x`
    Cross,
}

enum Item {
    Message { from: usize, to: usize, text: String, kind: Kind, arrow: bool },
    /// 블록 시작(alt/opt/loop/par/critical)
    BlockStart { tag: String, text: String },
    /// 같은 블록 안의 갈래(else/and/option)
    BlockElse { tag: String, text: String },
    BlockEnd,
    Note { from: usize, to: usize, text: String },
}

struct Doc {
    names: Vec<String>,
    actors: Vec<bool>,
    items: Vec<Item>,
    autonumber: bool,
}

pub fn render(src: &str, theme: &Theme, width: usize) -> Vec<Line> {
    let doc = parse(src);
    if doc.names.is_empty() {
        return Vec::new();
    }
    draw(&doc, theme, width)
}

fn parse(src: &str) -> Doc {
    let mut doc = Doc { names: Vec::new(), actors: Vec::new(), items: Vec::new(), autonumber: false };
    let mut alias: Vec<(String, usize)> = Vec::new();
    let mut note_buf: Option<(usize, usize, String)> = None;

    let ensure = |doc: &mut Doc, alias: &mut Vec<(String, usize)>, key: &str| -> usize {
        let key = key.trim();
        if let Some((_, i)) = alias.iter().find(|(k, _)| k == key) {
            return *i;
        }
        doc.names.push(label(key));
        doc.actors.push(false);
        alias.push((key.to_string(), doc.names.len() - 1));
        doc.names.len() - 1
    };

    for raw in clean_lines(src) {
        let t = raw.trim();
        let lower = t.to_ascii_lowercase();
        if lower.starts_with("sequencediagram") {
            continue;
        }
        if lower.starts_with("autonumber") {
            doc.autonumber = true;
            continue;
        }
        if lower.starts_with("participant ") || lower.starts_with("actor ") {
            let is_actor = lower.starts_with("actor ");
            let rest = t.split_once(char::is_whitespace).map(|x| x.1).unwrap_or("").trim();
            // `A as 이름` 형태
            let (key, name) = match split_as(rest) {
                Some((k, n)) => (k, n),
                None => (rest.to_string(), rest.to_string()),
            };
            let i = ensure(&mut doc, &mut alias, &key);
            doc.names[i] = label(&name);
            doc.actors[i] = is_actor;
            continue;
        }
        if lower.starts_with("note ") {
            if let Some((targets, text)) = t[5..].split_once(':') {
                let tl = targets.to_ascii_lowercase();
                let who = tl
                    .trim_start_matches("over")
                    .trim_start_matches("left of")
                    .trim_start_matches("right of")
                    .trim();
                let keys: Vec<&str> = targets[targets.len() - who.len()..].split(',').map(str::trim).collect();
                let a = ensure(&mut doc, &mut alias, keys[0]);
                let b = keys.get(1).map(|k| ensure(&mut doc, &mut alias, k)).unwrap_or(a);
                doc.items.push(Item::Note { from: a.min(b), to: a.max(b), text: label(text) });
            }
            continue;
        }
        if let Some(rest) = strip_kw(&lower, t, &["alt", "opt", "loop", "par", "critical", "rect", "break"]) {
            let tag = lower.split_whitespace().next().unwrap_or("").to_string();
            doc.items.push(Item::BlockStart { tag, text: label(&rest) });
            continue;
        }
        if let Some(rest) = strip_kw(&lower, t, &["else", "and", "option"]) {
            let tag = lower.split_whitespace().next().unwrap_or("").to_string();
            doc.items.push(Item::BlockElse { tag, text: label(&rest) });
            continue;
        }
        if lower == "end" {
            doc.items.push(Item::BlockEnd);
            continue;
        }
        if lower.starts_with("activate") || lower.starts_with("deactivate") || lower.starts_with("destroy") || lower.starts_with("box") || lower.starts_with("acc") {
            continue;
        }
        // 메시지
        if let Some((left, text)) = t.split_once(':')
            && let Some((a, b, kind, arrow)) = split_arrow(left)
        {
            let i = ensure(&mut doc, &mut alias, &a);
            let j = ensure(&mut doc, &mut alias, &b);
            doc.items.push(Item::Message { from: i, to: j, text: label(text), kind, arrow });
            continue;
        }
        let _ = &mut note_buf;
    }
    doc
}

fn split_as(s: &str) -> Option<(String, String)> {
    let lower = s.to_ascii_lowercase();
    let pos = lower.find(" as ")?;
    Some((s[..pos].trim().to_string(), s[pos + 4..].trim().to_string()))
}

fn strip_kw(lower: &str, orig: &str, kws: &[&str]) -> Option<String> {
    for k in kws {
        if lower == *k {
            return Some(String::new());
        }
        if let Some(rest) = lower.strip_prefix(k)
            && rest.starts_with(char::is_whitespace)
        {
            return Some(orig[k.len()..].trim().to_string());
        }
    }
    None
}

/// `A->>B` 꼴을 나눈다.
fn split_arrow(s: &str) -> Option<(String, String, Kind, bool)> {
    const PATTERNS: [(&str, Kind, bool); 12] = [
        ("-->>", Kind::Dashed, true),
        ("->>", Kind::Solid, true),
        ("--x", Kind::Dashed, true),
        ("-x", Kind::Solid, true),
        ("--)", Kind::Dashed, true),
        ("-)", Kind::Solid, true),
        ("-->", Kind::Dashed, true),
        ("->", Kind::Solid, true),
        ("--", Kind::Dashed, false),
        ("<<-->>", Kind::Dashed, true),
        ("<<->>", Kind::Solid, true),
        ("-", Kind::Solid, false),
    ];
    for (pat, kind, arrow) in PATTERNS {
        if let Some(p) = s.find(pat) {
            let a = s[..p].trim();
            let b = s[p + pat.len()..].trim();
            if a.is_empty() || b.is_empty() {
                continue;
            }
            let kind = if pat.ends_with('x') { Kind::Cross } else { kind };
            return Some((a.to_string(), b.to_string(), kind, arrow));
        }
    }
    None
}

fn draw(doc: &Doc, theme: &Theme, width: usize) -> Vec<Line> {
    let n = doc.names.len();
    // 참가자 상자 폭
    let head: Vec<String> = doc
        .names
        .iter()
        .enumerate()
        .map(|(i, s)| if doc.actors[i] { format!("☺ {s}") } else { s.clone() })
        .collect();
    let mut colw: Vec<usize> = head.iter().map(|s| width_of(s) + 4).collect();
    // 메시지 글자가 들어갈 만큼 칸 사이를 넓힌다.
    let mut gap = vec![4usize; n.saturating_sub(1)];
    for it in &doc.items {
        if let Item::Message { from, to, text, .. } = it {
            let (a, b) = (*from.min(to), *from.max(to));
            let need = width_of(&truncate(text, 40)) + 4;
            if a == b {
                continue;
            }
            let span: usize = (a..b).map(|k| gap[k]).sum::<usize>() + (a + 1..b).map(|k| colw[k]).sum::<usize>();
            if span < need {
                let add = need - span;
                let each = add.div_ceil(b - a);
                for g in &mut gap[a..b] {
                    *g += each;
                }
            }
        }
    }
    // 폭이 넘치면 간격을 줄인다.
    let total = |colw: &[usize], gap: &[usize]| colw.iter().sum::<usize>() + gap.iter().sum::<usize>() + 2;
    while total(&colw, &gap) > width && gap.iter().any(|g| *g > 4) {
        let m = gap.iter().copied().max().unwrap_or(0);
        for g in gap.iter_mut() {
            if *g == m {
                *g -= 1;
            }
        }
    }
    while total(&colw, &gap) > width && colw.iter().any(|w| *w > 7) {
        let m = colw.iter().copied().max().unwrap_or(0);
        for w in colw.iter_mut() {
            if *w == m {
                *w -= 1;
            }
        }
    }

    let mut x = vec![0usize; n];
    let mut cx = 1usize;
    for i in 0..n {
        x[i] = cx + colw[i] / 2;
        cx += colw[i] + gap.get(i).copied().unwrap_or(0);
    }
    let canvas_w = cx + 2;

    // 높이 계산: 머리 3줄 + 항목별 줄 수
    let mut h = 4usize;
    for it in &doc.items {
        h += match it {
            Item::Message { .. } => 2,
            Item::Note { .. } => 3,
            Item::BlockStart { .. } | Item::BlockElse { .. } => 2,
            Item::BlockEnd => 1,
        };
    }
    h += 3;

    let mut c = Canvas::new(canvas_w, h);
    let border = theme.diagram_border;
    let life = theme.diagram_edge;
    let note_st = theme.diagram_note;

    // 참가자 머리
    for i in 0..n {
        let w = colw[i];
        let x0 = x[i] - w / 2;
        c.rect(x0, 0, w, 3, border, doc.actors[i]);
        let t = truncate(&head[i], w.saturating_sub(2));
        c.text(x0 + 1 + (w.saturating_sub(2) - width_of(&t)) / 2, 1, &t, theme.diagram_title);
    }

    // 생명선을 먼저 긋고, 메시지·메모를 그 위에 덮어쓴다.
    let content: usize = doc
        .items
        .iter()
        .map(|it| match it {
            Item::Message { .. } => 2,
            Item::Note { .. } => 3,
            Item::BlockStart { .. } | Item::BlockElse { .. } => 2,
            Item::BlockEnd => 1,
        })
        .sum();
    for xi in &x {
        c.vline(*xi, 3, 3 + content, '│', life);
    }

    let mut y = 3usize;
    let mut blocks: Vec<(usize, usize)> = Vec::new(); // (시작 y, 왼쪽 x)
    let mut num = 1usize;
    for it in &doc.items {
        match it {
            Item::Message { from, to, text, kind, arrow } => {
                let mut t = truncate(text, 40);
                if doc.autonumber {
                    t = format!("{num}. {t}");
                    num += 1;
                }
                if from == to {
                    // 자기 자신에게 보내는 메시지
                    c.text(x[*from] + 3, y, &t, note_st);
                    c.set(x[*from] + 1, y + 1, '↺', life);
                    y += 2;
                    continue;
                }
                let (a, b) = (x[*from].min(x[*to]), x[*from].max(x[*to]));
                let mid = (a + b) / 2;
                let tw = width_of(&t);
                c.text(mid.saturating_sub(tw / 2), y, &t, note_st);
                let ch = if *kind == Kind::Dashed { '╌' } else { '─' };
                c.hline(a + 1, b - 1, y + 1, ch, life);
                if *arrow {
                    let head_ch = if *kind == Kind::Cross {
                        '✕'
                    } else if x[*to] > x[*from] {
                        '▶'
                    } else {
                        '◀'
                    };
                    c.set(x[*to], y + 1, head_ch, life);
                }
                y += 2;
            }
            Item::Note { from, to, text } => {
                let t = truncate(&text.replace('\n', " "), 44);
                let tw = width_of(&t) + 2;
                let mid = (x[*from] + x[*to]) / 2;
                let x0 = mid.saturating_sub(tw / 2).min(canvas_w.saturating_sub(tw + 2));
                c.clear_rect(x0, y, tw + 2, 3);
                c.rect(x0, y, tw + 2, 3, note_st, true);
                c.text(x0 + 2, y + 1, &t, note_st);
                y += 3;
            }
            Item::BlockStart { tag, text } => {
                blocks.push((y, 0));
                let t = if text.is_empty() { tag.to_string() } else { format!("{tag}: {}", truncate(text, 34)) };
                c.text(3, y, &t, note_st);
                y += 2;
            }
            Item::BlockElse { tag, text } => {
                let t = if text.is_empty() { tag.to_string() } else { format!("{tag}: {}", truncate(text, 34)) };
                c.hline(2, canvas_w.saturating_sub(3), y, '╌', note_st);
                c.text(3, y, &format!(" {t} "), note_st);
                y += 2;
            }
            Item::BlockEnd => {
                if let Some((y0, _)) = blocks.pop() {
                    c.vline(1, y0 + 1, y.saturating_sub(1), '╎', note_st);
                    c.set(1, y0, '╭', note_st);
                    c.set(2, y0, '╴', note_st);
                    c.set(1, y, '╰', note_st);
                    c.set(2, y, '╴', note_st);
                }
                y += 1;
            }
        }
    }

    // 끝 상자
    for i in 0..n {
        let w = colw[i];
        let x0 = x[i] - w / 2;
        c.clear_rect(x0, y + 1, w, 3);
        c.rect(x0, y + 1, w, 3, border, doc.actors[i]);
        let t = truncate(&head[i], w.saturating_sub(2));
        c.text(x0 + 1 + (w.saturating_sub(2) - width_of(&t)) / 2, y + 2, &t, theme.diagram_title);
    }
    c.grow_to(y + 4);
    c.into_lines()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out(src: &str) -> Vec<String> {
        render(src, &Theme::notty(), 80).iter().map(Line::plain).collect()
    }

    #[test]
    fn draws_participants_and_message() {
        let o = out("sequenceDiagram\n  participant A as 앨리스\n  participant B as 밥\n  A->>B: 안녕\n");
        assert!(o[1].contains("앨리스") && o[1].contains("밥"));
        assert!(o.iter().any(|l| l.contains("안녕")));
        assert!(o.iter().any(|l| l.contains('▶')));
    }

    #[test]
    fn dashed_reply_points_left() {
        let o = out("sequenceDiagram\n A->>B: q\n B-->>A: r\n");
        assert!(o.iter().any(|l| l.contains('◀') && l.contains('╌')));
    }

    #[test]
    fn alt_block_is_framed() {
        let o = out("sequenceDiagram\n A->>B: x\n alt 성공\n  B-->>A: ok\n else 실패\n  B-->>A: no\n end\n");
        assert!(o.iter().any(|l| l.contains("alt: 성공")));
        assert!(o.iter().any(|l| l.contains("else: 실패")));
    }

    #[test]
    fn note_is_boxed() {
        let o = out("sequenceDiagram\n A->>B: x\n Note over A,B: 메모\n");
        assert!(o.iter().any(|l| l.contains("메모")));
    }

    #[test]
    fn autonumber_prefixes_messages() {
        let o = out("sequenceDiagram\n autonumber\n A->>B: 하나\n A->>B: 둘\n");
        assert!(o.iter().any(|l| l.contains("1. 하나")));
        assert!(o.iter().any(|l| l.contains("2. 둘")));
    }
}
