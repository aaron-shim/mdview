//! `classDiagram`: 클래스 상자(이름·속성·메서드)와 관계선을 그린다.

use super::graph::{Arrow, DIVIDER, GEdge, GNode, Graph, Shape};
use super::parse::{clean_lines, label};
use std::collections::HashMap;

#[derive(Default)]
struct Body {
    stereotype: Option<String>,
    fields: Vec<String>,
    methods: Vec<String>,
}

pub fn parse(src: &str) -> Graph {
    let mut g = Graph::default();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut bodies: Vec<Body> = Vec::new();
    let mut open: Option<usize> = None;

    for raw in clean_lines(src) {
        let t = raw.trim();
        let lower = t.to_ascii_lowercase();
        if lower.starts_with("classdiagram") || lower.starts_with("direction") || lower.starts_with("acc") || lower.starts_with("click") || lower.starts_with("style") || lower.starts_with("cssclass") {
            continue;
        }
        if let Some(i) = open {
            if t == "}" {
                open = None;
                continue;
            }
            add_member(&mut bodies[i], t);
            continue;
        }
        if let Some(rest) = t.strip_prefix("class ") {
            let rest = rest.trim();
            let (name, tail) = match rest.find(['{', ':']) {
                Some(p) => (rest[..p].trim(), rest[p..].trim()),
                None => (rest, ""),
            };
            let name = name.split_whitespace().next().unwrap_or(name);
            let i = class(&mut g, &mut index, &mut bodies, name);
            if tail.starts_with('{') {
                open = Some(i);
            } else if let Some(m) = tail.strip_prefix(':') {
                add_member(&mut bodies[i], m.trim());
            }
            continue;
        }
        // `ClassName : +method()` 형태
        if let Some((name, member)) = t.split_once(':')
            && !t.contains("--")
            && !t.contains("..")
            && is_ident(name.trim())
        {
            let i = class(&mut g, &mut index, &mut bodies, name.trim());
            add_member(&mut bodies[i], member.trim());
            continue;
        }
        // 관계
        if let Some((a, rel, b, lab)) = split_relation(t) {
            let (from, to, head, dashed) = arrow_of(&rel, &a, &b);
            let i = class(&mut g, &mut index, &mut bodies, &from);
            let j = class(&mut g, &mut index, &mut bodies, &to);
            let mut e = GEdge::new(i, j);
            e.head = head;
            e.dashed = dashed;
            e.label = lab;
            g.edges.push(e);
        }
    }

    // 본문을 상자 줄로 옮긴다.
    for (i, b) in bodies.iter().enumerate() {
        let mut lines = vec![g.nodes[i].id.clone()];
        if let Some(s) = &b.stereotype {
            lines.push(format!("«{s}»"));
        }
        if !b.fields.is_empty() {
            lines.push(DIVIDER.to_string());
            lines.extend(b.fields.iter().cloned());
        }
        if !b.methods.is_empty() {
            lines.push(DIVIDER.to_string());
            lines.extend(b.methods.iter().cloned());
        }
        g.nodes[i].lines = lines;
    }
    g
}

fn is_ident(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '~')
}

fn class(g: &mut Graph, index: &mut HashMap<String, usize>, bodies: &mut Vec<Body>, name: &str) -> usize {
    let name = name.trim().trim_matches('"');
    let name = name.split(['~', '[']).next().unwrap_or(name).trim();
    if let Some(&i) = index.get(name) {
        return i;
    }
    let mut n = GNode::new(name, name, Shape::Rect);
    n.header = true;
    n.left_align = true;
    g.nodes.push(n);
    bodies.push(Body::default());
    index.insert(name.to_string(), g.nodes.len() - 1);
    g.nodes.len() - 1
}

fn add_member(b: &mut Body, line: &str) {
    let t = line.trim();
    if t.is_empty() {
        return;
    }
    if let Some(inner) = t.strip_prefix("<<").and_then(|s| s.strip_suffix(">>")) {
        b.stereotype = Some(inner.trim().to_string());
        return;
    }
    let t = label(t);
    if t.contains('(') { b.methods.push(t) } else { b.fields.push(t) }
}

/// `A <|-- B : 라벨` 을 (A, 관계, B, 라벨)로 나눈다.
fn split_relation(t: &str) -> Option<(String, String, String, String)> {
    let (body, lab) = match t.split_once(':') {
        Some((a, b)) => (a.trim(), label(b)),
        None => (t.trim(), String::new()),
    };
    let chars: Vec<char> = body.chars().collect();
    let link = |c: char| matches!(c, '-' | '.');
    let deco = |c: char| matches!(c, '<' | '>' | '|' | '*' | 'o');
    let mut i = 0;
    while i + 1 < chars.len() {
        if link(chars[i]) && link(chars[i + 1]) {
            let mut lstart = i;
            while lstart > 0 && deco(chars[lstart - 1]) && lstart + 2 > i {
                lstart -= 1;
            }
            // `<|`, `*`, `o` 처럼 최대 두 글자
            let mut s = lstart;
            while s > 0 && i - s < 2 && deco(chars[s - 1]) {
                s -= 1;
            }
            lstart = s;
            let mut rend = i + 2;
            while rend < chars.len() && link(chars[rend]) {
                rend += 1;
            }
            while rend < chars.len() && rend < i + 6 && deco(chars[rend]) {
                rend += 1;
            }
            let left: String = chars[..lstart].iter().collect();
            let rel: String = chars[lstart..rend].iter().collect();
            let right: String = chars[rend..].iter().collect();
            if left.trim().is_empty() || right.trim().is_empty() {
                return None;
            }
            return Some((strip_mult(&left), rel, strip_mult(&right), lab));
        }
        i += 1;
    }
    None
}

/// `A "1"` 처럼 붙은 다중도 표기를 뗀다.
fn strip_mult(s: &str) -> String {
    let t = s.trim();
    match t.find('"') {
        Some(p) => t[..p].trim().to_string(),
        None => t.to_string(),
    }
}

/// 관계 기호 → (출발, 도착, 머리 모양, 점선 여부).
fn arrow_of(rel: &str, a: &str, b: &str) -> (String, String, Arrow, bool) {
    let dashed = rel.contains("..");
    let (l, r) = (a.to_string(), b.to_string());
    let head = |c: &str| match c {
        "<|" | "|>" => Arrow::Hollow,
        "*" => Arrow::Diamond,
        "o" => Arrow::HollowDiamond,
        "<" | ">" => Arrow::Open,
        _ => Arrow::None,
    };
    let lead: String = rel.chars().take_while(|c| !matches!(c, '-' | '.')).collect();
    let tail: String = {
        let t: String = rel.chars().rev().take_while(|c| !matches!(c, '-' | '.')).collect();
        t.chars().rev().collect()
    };
    if !lead.is_empty() {
        // 머리가 왼쪽에 있으면 방향을 뒤집는다.
        (r, l, head(&lead), dashed)
    } else if !tail.is_empty() {
        (l, r, head(&tail), dashed)
    } else {
        (l, r, Arrow::None, dashed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_body_splits_fields_and_methods() {
        let g = parse("classDiagram\n class A {\n  -Repo repo\n  +run() void\n }\n");
        assert_eq!(g.nodes[0].lines, vec!["A", DIVIDER, "-Repo repo", DIVIDER, "+run() void"]);
    }

    #[test]
    fn stereotype_is_shown() {
        let g = parse("classDiagram\n class P {\n  <<interface>>\n  +go() void\n }\n");
        assert_eq!(g.nodes[0].lines[1], "«interface»");
    }

    #[test]
    fn inheritance_points_at_parent() {
        let g = parse("classDiagram\n Repo <|.. JpaRepo\n");
        // 화살표는 부모(Repo)를 가리키므로 자식에서 부모로 향한다.
        assert_eq!(g.nodes[g.edges[0].from].id, "JpaRepo");
        assert_eq!(g.nodes[g.edges[0].to].id, "Repo");
        assert_eq!(g.edges[0].head, Arrow::Hollow);
        assert!(g.edges[0].dashed);
    }

    #[test]
    fn association_keeps_direction_and_label() {
        let g = parse("classDiagram\n Svc --> Repo : 사용\n");
        assert_eq!(g.nodes[g.edges[0].from].id, "Svc");
        assert_eq!(g.edges[0].head, Arrow::Open);
        assert_eq!(g.edges[0].label, "사용");
    }

    #[test]
    fn composition_uses_diamond() {
        let g = parse("classDiagram\n Order *-- Item\n");
        assert_eq!(g.edges[0].head, Arrow::Diamond);
        assert_eq!(g.nodes[g.edges[0].to].id, "Order");
    }
}
