//! `flowchart` / `graph` 파서.

use super::graph::{Arrow, GEdge, GNode, Graph, Group, Shape};
use super::parse::{clean_lines, label};
use std::collections::HashMap;

/// 노드 참조 하나: 아이디와 (있다면) 모양·라벨.
struct NodeRef {
    id: String,
    label: Option<String>,
    shape: Shape,
}

/// 연결선 하나.
struct Link {
    label: String,
    dashed: bool,
    thick: bool,
    head: Arrow,
    tail: Arrow,
}

pub fn parse(src: &str) -> Graph {
    let mut g = Graph::default();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut stack: Vec<usize> = Vec::new(); // 열려 있는 subgraph

    for line in clean_lines(src) {
        let t = line.trim();
        let lower = t.to_ascii_lowercase();
        if lower.starts_with("flowchart") || lower.starts_with("graph ") || lower == "graph" {
            continue;
        }
        if lower.starts_with("subgraph") {
            let rest = t[t.find(|c: char| c.is_whitespace()).unwrap_or(t.len())..].trim();
            let title = subgraph_title(rest);
            g.groups.push(Group { title, members: Vec::new() });
            stack.push(g.groups.len() - 1);
            continue;
        }
        if lower == "end" {
            stack.pop();
            continue;
        }
        if lower.starts_with("direction")
            || lower.starts_with("classdef")
            || lower.starts_with("class ")
            || lower.starts_with("style ")
            || lower.starts_with("linkstyle")
            || lower.starts_with("click ")
            || lower.starts_with("accTitle")
            || lower.starts_with("acctitle")
        {
            continue;
        }
        for stmt in t.split(';').map(str::trim).filter(|s| !s.is_empty()) {
            parse_statement(stmt, &mut g, &mut index, stack.last().copied());
        }
    }
    g
}

fn subgraph_title(rest: &str) -> String {
    // `Id["제목"]` 또는 `제목`
    if let Some(open) = rest.find(['[', '(']) {
        let close = rest.rfind([']', ')']).unwrap_or(rest.len());
        if close > open {
            return label(&rest[open + 1..close]);
        }
    }
    label(rest)
}

/// `A --> B --> C` 같은 한 문장을 읽는다.
fn parse_statement(stmt: &str, g: &mut Graph, index: &mut HashMap<String, usize>, group: Option<usize>) {
    let chars: Vec<char> = stmt.chars().collect();
    let mut i = 0usize;
    let Some(first) = read_node(&chars, &mut i) else { return };
    let mut prev = intern(g, index, first, group);
    let mut any = false;
    loop {
        skip_ws(&chars, &mut i);
        let Some(link) = read_link(&chars, &mut i) else { break };
        skip_ws(&chars, &mut i);
        let Some(nref) = read_node(&chars, &mut i) else { break };
        let cur = intern(g, index, nref, group);
        let mut e = GEdge::new(prev, cur);
        e.label = link.label;
        e.dashed = link.dashed;
        e.thick = link.thick;
        e.head = link.head;
        e.tail = link.tail;
        g.edges.push(e);
        prev = cur;
        any = true;
    }
    let _ = any;
}

fn skip_ws(c: &[char], i: &mut usize) {
    while *i < c.len() && c[*i].is_whitespace() {
        *i += 1;
    }
}

/// 노드 참조를 읽는다.
fn read_node(c: &[char], i: &mut usize) -> Option<NodeRef> {
    skip_ws(c, i);
    let start = *i;
    while *i < c.len() && is_id_char(c[*i]) {
        *i += 1;
    }
    if *i == start {
        return None;
    }
    let id: String = c[start..*i].iter().collect();
    // 모양 구분자
    let (shape, open, close) = match (c.get(*i), c.get(*i + 1)) {
        (Some('['), Some('[')) => (Shape::Subroutine, 2, "]]"),
        (Some('['), Some('(')) => (Shape::Cylinder, 2, ")]"),
        (Some('('), Some('[')) => (Shape::Round, 2, "])"),
        (Some('('), Some('(')) => (Shape::Circle, 2, "))"),
        (Some('{'), Some('{')) => (Shape::Round, 2, "}}"),
        (Some('['), Some('/')) => (Shape::Round, 2, "/]"),
        (Some('['), Some('\\')) => (Shape::Round, 2, "\\]"),
        (Some('['), _) => (Shape::Rect, 1, "]"),
        (Some('('), _) => (Shape::Round, 1, ")"),
        (Some('{'), _) => (Shape::Diamond, 1, "}"),
        (Some('>'), _) => (Shape::Round, 1, "]"),
        _ => return Some(NodeRef { id, label: None, shape: Shape::Rect }),
    };
    *i += open;
    let body_start = *i;
    let closers: Vec<char> = close.chars().collect();
    let mut quote: Option<char> = None;
    let mut end = None;
    while *i < c.len() {
        let ch = c[*i];
        match quote {
            Some(q) => {
                if ch == q {
                    quote = None;
                }
            }
            None => {
                if ch == '"' || ch == '\'' {
                    quote = Some(ch);
                } else if ch == closers[0] && (closers.len() == 1 || c.get(*i + 1) == Some(&closers[1])) {
                    end = Some(*i);
                    break;
                }
            }
        }
        *i += 1;
    }
    let body_end = end.unwrap_or(c.len());
    let text: String = c[body_start..body_end].iter().collect();
    *i = body_end + closers.len();
    Some(NodeRef { id, label: Some(label(&text)), shape })
}

fn is_id_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '-' && false || c == '.' || c == '#'
}

/// 연결선을 읽는다. `-->`, `-.->`, `==>`, `-- 글자 -->`, `-->|글자|` 등.
fn read_link(c: &[char], i: &mut usize) -> Option<Link> {
    skip_ws(c, i);
    let start = *i;
    if !matches!(c.get(*i), Some('-') | Some('=') | Some('<') | Some('~') | Some('.') | Some('o') | Some('x')) {
        return None;
    }
    // 시작 머리(`<--`, `x--`, `o--`)
    let mut tail = Arrow::None;
    if matches!(c.get(*i), Some('<')) {
        tail = Arrow::Open;
        *i += 1;
    } else if matches!(c.get(*i), Some('o') | Some('x')) && matches!(c.get(*i + 1), Some('-') | Some('=')) {
        tail = if c[*i] == 'x' { Arrow::Cross } else { Arrow::HollowDiamond };
        *i += 1;
    }
    let run_start = *i;
    while matches!(c.get(*i), Some('-') | Some('=') | Some('.') | Some('~')) {
        *i += 1;
    }
    if *i == run_start {
        *i = start;
        return None;
    }
    let run1: String = c[run_start..*i].iter().collect();
    let mut head = Arrow::None;
    let mut text = String::new();
    match c.get(*i) {
        Some('>') => {
            head = Arrow::Open;
            *i += 1;
        }
        Some('x') => {
            head = Arrow::Cross;
            *i += 1;
        }
        Some('o') => {
            head = Arrow::HollowDiamond;
            *i += 1;
        }
        _ => {
            // `-- 글자 -->` 형태: 다음 연결선 조각까지가 라벨이다.
            let save = *i;
            let mut j = *i;
            let mut buf = String::new();
            while j < c.len() && !matches!(c[j], '-' | '=' | '.' | '~') {
                buf.push(c[j]);
                j += 1;
            }
            let mut k = j;
            while matches!(c.get(k), Some('-') | Some('=') | Some('.') | Some('~')) {
                k += 1;
            }
            if k > j && !buf.trim().is_empty() {
                text = buf.trim().to_string();
                *i = k;
                match c.get(*i) {
                    Some('>') => {
                        head = Arrow::Open;
                        *i += 1;
                    }
                    Some('x') => {
                        head = Arrow::Cross;
                        *i += 1;
                    }
                    Some('o') => {
                        head = Arrow::HollowDiamond;
                        *i += 1;
                    }
                    _ => {}
                }
            } else {
                *i = save;
            }
        }
    }
    // `|글자|` 형태의 라벨
    if matches!(c.get(*i), Some('|')) {
        *i += 1;
        let s = *i;
        while *i < c.len() && c[*i] != '|' {
            *i += 1;
        }
        text = c[s..*i].iter().collect();
        if *i < c.len() {
            *i += 1;
        }
    }
    Some(Link {
        label: label(&text).replace('\n', " "),
        dashed: run1.contains('.') || run1.contains('~'),
        thick: run1.contains('='),
        head,
        tail,
    })
}

/// 노드를 등록하거나 이미 있는 노드를 갱신한다.
fn intern(g: &mut Graph, index: &mut HashMap<String, usize>, r: NodeRef, group: Option<usize>) -> usize {
    let idx = match index.get(&r.id) {
        Some(&i) => i,
        None => {
            let text = r.label.clone().unwrap_or_else(|| r.id.clone());
            let mut n = GNode::new(r.id.clone(), &text, r.shape);
            n.group = group;
            g.nodes.push(n);
            index.insert(r.id.clone(), g.nodes.len() - 1);
            g.nodes.len() - 1
        }
    };
    // 뒤에 나온 라벨·모양 정의를 반영한다.
    if let Some(text) = r.label {
        g.nodes[idx].lines = text.lines().map(str::to_string).collect();
        g.nodes[idx].shape = r.shape;
    }
    if let Some(gi) = group
        && g.nodes[idx].group.is_none()
    {
        g.nodes[idx].group = Some(gi);
    }
    if let Some(gi) = g.nodes[idx].group
        && !g.groups[gi].members.contains(&idx)
    {
        g.groups[gi].members.push(idx);
    }
    idx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_chain_and_labels() {
        let g = parse("flowchart TB\n  A[시작] --> B{판단}\n  B -->|예| C((끝))\n");
        assert_eq!(g.nodes.len(), 3);
        assert_eq!(g.nodes[0].lines, vec!["시작"]);
        assert_eq!(g.nodes[1].shape, Shape::Diamond);
        assert_eq!(g.edges.len(), 2);
        assert_eq!(g.edges[1].label, "예");
    }

    #[test]
    fn parses_dotted_link_with_inline_text() {
        let g = parse("graph TD\n  A -.읽기 전용.-> B\n  C -. 복제 .-> D\n");
        assert!(g.edges[0].dashed);
        assert_eq!(g.edges[0].label, "읽기 전용");
        assert_eq!(g.edges[1].label, "복제");
    }

    #[test]
    fn parses_subgraph_membership() {
        let g = parse("flowchart TB\n subgraph Edge[\"엣지\"]\n  CDN[CDN]\n  ALB[ALB]\n end\n CDN --> ALB\n");
        assert_eq!(g.groups.len(), 1);
        assert_eq!(g.groups[0].title, "엣지");
        assert_eq!(g.groups[0].members.len(), 2);
    }

    #[test]
    fn shapes_are_recognised() {
        let g = parse("graph LR\n A[(DB)] --> B[[Sub]]\n B --> C([Stad])\n");
        assert_eq!(g.nodes[0].shape, Shape::Cylinder);
        assert_eq!(g.nodes[1].shape, Shape::Subroutine);
        assert_eq!(g.nodes[2].shape, Shape::Round);
    }

    #[test]
    fn multiline_label_splits() {
        let g = parse("flowchart TB\n A[\"위<br/>아래\"] --> B\n");
        assert_eq!(g.nodes[0].lines, vec!["위", "아래"]);
    }

    #[test]
    fn thick_and_plain_links() {
        let g = parse("graph TD\n A ==> B\n B --- C\n");
        assert!(g.edges[0].thick);
        assert_eq!(g.edges[1].head, Arrow::None);
    }
}
