//! `stateDiagram` / `stateDiagram-v2`: 상태 전이를 계층 그래프로 그린다.

use super::graph::{Arrow, GEdge, GNode, Graph, Group, Shape};
use super::parse::{clean_lines, label};
use std::collections::HashMap;

pub fn parse(src: &str) -> Graph {
    let mut g = Graph::default();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut stack: Vec<usize> = Vec::new();
    let mut note: Option<(String, Vec<String>)> = None;
    let mut start: Option<usize> = None;
    let mut end: Option<usize> = None;

    for raw in clean_lines(src) {
        let t = raw.trim();
        let lower = t.to_ascii_lowercase();

        // 여러 줄 note
        if let Some((target, body)) = note.as_mut() {
            if lower == "end note" {
                let text = body.join("\n");
                let target = target.clone();
                note = None;
                attach_note(&mut g, &mut index, &target, &text, stack.last().copied());
            } else {
                body.push(t.to_string());
            }
            continue;
        }
        if lower.starts_with("statediagram") || lower.starts_with("direction") || lower.starts_with("classdef") || lower.starts_with("class ") || lower.starts_with("acc") {
            continue;
        }
        if lower.starts_with("note ") {
            let rest = &t[5..];
            let who = rest
                .trim_start_matches("right of")
                .trim_start_matches("left of")
                .trim_start_matches("over")
                .trim();
            match who.split_once(':') {
                Some((target, text)) => attach_note(&mut g, &mut index, target.trim(), &label(text), stack.last().copied()),
                None => note = Some((who.to_string(), Vec::new())),
            }
            continue;
        }
        // 합성 상태: `state 이름 {`
        if lower.starts_with("state ") && t.ends_with('{') {
            let inner = t[6..t.len() - 1].trim();
            let title = state_title(inner);
            g.groups.push(Group { title, members: Vec::new() });
            stack.push(g.groups.len() - 1);
            continue;
        }
        if t == "}" || lower == "end" {
            stack.pop();
            continue;
        }
        // `state "설명" as id` / `state id : 설명`
        if lower.starts_with("state ") {
            let rest = t[6..].trim();
            if let Some((desc, id)) = rest.split_once(" as ") {
                let i = node(&mut g, &mut index, id.trim(), stack.last().copied(), &mut start, &mut end);
                if let Some(i) = i {
                    g.nodes[i].lines = label(desc).lines().map(str::to_string).collect();
                }
            } else if let Some((id, desc)) = rest.split_once(':') {
                let i = node(&mut g, &mut index, id.trim(), stack.last().copied(), &mut start, &mut end);
                if let Some(i) = i {
                    g.nodes[i].lines = label(desc).lines().map(str::to_string).collect();
                }
            } else {
                node(&mut g, &mut index, rest, stack.last().copied(), &mut start, &mut end);
            }
            continue;
        }
        // 전이
        let Some(p) = t.find("-->") else {
            continue;
        };
        let from = t[..p].trim().to_string();
        let rest = t[p + 3..].trim();
        let (to, lab) = match rest.split_once(':') {
            Some((a, b)) => (a.trim().to_string(), label(b)),
            None => (rest.to_string(), String::new()),
        };
        let group = stack.last().copied();
        let (Some(a), Some(b)) = (
            node(&mut g, &mut index, &from, group, &mut start, &mut end),
            node(&mut g, &mut index, &to, group, &mut start, &mut end),
        ) else {
            continue;
        };
        let mut e = GEdge::new(a, b);
        e.label = lab;
        g.edges.push(e);
    }
    g
}

fn state_title(s: &str) -> String {
    match s.split_once(" as ") {
        Some((desc, _)) => label(desc),
        None => label(s),
    }
}

/// 상태 노드를 만들거나 찾는다. `[*]`는 시작·끝 표시로 바꾼다.
fn node(
    g: &mut Graph,
    index: &mut HashMap<String, usize>,
    name: &str,
    group: Option<usize>,
    start: &mut Option<usize>,
    end: &mut Option<usize>,
) -> Option<usize> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    if name == "[*]" {
        // 처음 나오면 시작점, 그 다음은 끝점으로 본다.
        let is_start = start.is_none();
        if let Some(i) = if is_start { *start } else { *end } {
            return Some(i);
        }
        let mut n = GNode::new(format!("__terminal{}", g.nodes.len()), if is_start { "●" } else { "◉" }, Shape::Circle);
        n.group = group;
        g.nodes.push(n);
        let i = g.nodes.len() - 1;
        if is_start {
            *start = Some(i);
        } else {
            *end = Some(i);
        }
        add_member(g, group, i);
        return Some(i);
    }
    let (id, desc) = match name.split_once(':') {
        Some((a, b)) => (a.trim(), Some(label(b))),
        None => (name, None),
    };
    let i = match index.get(id) {
        Some(&i) => i,
        None => {
            let mut n = GNode::new(id, &label(id), Shape::Round);
            n.group = group;
            g.nodes.push(n);
            index.insert(id.to_string(), g.nodes.len() - 1);
            g.nodes.len() - 1
        }
    };
    if let Some(d) = desc {
        g.nodes[i].lines = d.lines().map(str::to_string).collect();
    }
    if g.nodes[i].group.is_none() {
        g.nodes[i].group = group;
    }
    add_member(g, g.nodes[i].group, i);
    Some(i)
}

fn add_member(g: &mut Graph, group: Option<usize>, i: usize) {
    if let Some(gi) = group
        && !g.groups[gi].members.contains(&i)
    {
        g.groups[gi].members.push(i);
    }
}

/// 메모는 점선으로 이어진 별도 상자로 그린다.
fn attach_note(g: &mut Graph, index: &mut HashMap<String, usize>, target: &str, text: &str, group: Option<usize>) {
    let Some(&t) = index.get(target.trim()) else { return };
    let mut n = GNode::new(format!("__note{}", g.nodes.len()), text, Shape::Round);
    n.group = group;
    g.nodes.push(n);
    let i = g.nodes.len() - 1;
    add_member(g, group, i);
    let mut e = GEdge::new(t, i);
    e.dashed = true;
    e.head = Arrow::None;
    g.edges.push(e);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_and_end_markers() {
        let g = parse("stateDiagram-v2\n [*] --> A\n A --> [*]\n");
        assert_eq!(g.nodes[0].lines, vec!["●"]);
        assert_eq!(g.nodes.last().unwrap().lines, vec!["◉"]);
        assert_eq!(g.edges.len(), 2);
    }

    #[test]
    fn transition_label_is_kept() {
        let g = parse("stateDiagram-v2\n 작성중 --> 제출완료 : 제출\n");
        assert_eq!(g.edges[0].label, "제출");
        assert_eq!(g.nodes[0].lines, vec!["작성중"]);
    }

    #[test]
    fn note_becomes_dashed_box() {
        let g = parse("stateDiagram-v2\n A --> B\n note right of B\n   메모\n end note\n");
        assert!(g.edges.iter().any(|e| e.dashed && e.head == Arrow::None));
        assert!(g.nodes.iter().any(|n| n.lines.contains(&"메모".to_string())));
    }

    #[test]
    fn composite_state_becomes_group() {
        let g = parse("stateDiagram-v2\n state 처리중 {\n  A --> B\n }\n");
        assert_eq!(g.groups.len(), 1);
        assert_eq!(g.groups[0].title, "처리중");
        assert_eq!(g.groups[0].members.len(), 2);
    }
}
