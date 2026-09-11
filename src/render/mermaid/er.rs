//! `erDiagram`: 엔터티 상자와 관계선을 그린다.

use super::graph::{Arrow, DIVIDER, GEdge, GNode, Graph, Shape};
use super::parse::{clean_lines, label};
use std::collections::HashMap;

pub fn parse(src: &str) -> Graph {
    let mut g = Graph::default();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut open: Option<usize> = None;

    for raw in clean_lines(src) {
        let t = raw.trim();
        let lower = t.to_ascii_lowercase();
        if lower.starts_with("erdiagram") || lower.starts_with("direction") || lower.starts_with("acc") {
            continue;
        }
        // 속성 블록
        if let Some(i) = open {
            if t == "}" {
                open = None;
                continue;
            }
            g.nodes[i].lines.push(attribute(t));
            continue;
        }
        if let Some(name) = t.strip_suffix('{') {
            let i = entity(&mut g, &mut index, name.trim());
            if g.nodes[i].lines.len() == 1 {
                g.nodes[i].lines.push(DIVIDER.to_string());
            }
            open = Some(i);
            continue;
        }
        // 관계: `A ||--o{ B : "라벨"`
        let Some((left, rel, right)) = split_relation(t) else { continue };
        let (right, lab) = match right.split_once(':') {
            Some((r, l)) => (r.trim().to_string(), label(l)),
            None => (right, String::new()),
        };
        let a = entity(&mut g, &mut index, &left);
        let b = entity(&mut g, &mut index, &right);
        let mut e = GEdge::new(a, b);
        e.label = lab;
        e.head = Arrow::None;
        e.dashed = rel.contains("..");
        e.tail_tag = cardinality(&rel, true);
        e.head_tag = cardinality(&rel, false);
        g.edges.push(e);
    }
    g
}

/// `이름 { ` 또는 관계에서 나온 엔터티.
fn entity(g: &mut Graph, index: &mut HashMap<String, usize>, name: &str) -> usize {
    let name = name.trim().trim_matches('"');
    if let Some(&i) = index.get(name) {
        return i;
    }
    let mut n = GNode::new(name, name, Shape::Rect);
    n.header = true;
    n.left_align = true;
    g.nodes.push(n);
    index.insert(name.to_string(), g.nodes.len() - 1);
    g.nodes.len() - 1
}

/// `bigint member_id PK "설명"` → `member_id  bigint  PK`
fn attribute(line: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    let mut comment = String::new();
    let mut rest = line.trim();
    if let Some(q) = rest.find('"') {
        comment = label(&rest[q..]);
        rest = rest[..q].trim();
    }
    parts.extend(rest.split_whitespace());
    let ty = parts.first().copied().unwrap_or("");
    let name = parts.get(1).copied().unwrap_or("");
    let key = parts[2.min(parts.len())..].join(" ");
    let mut out = format!("{name} : {ty}");
    if !key.is_empty() {
        out.push_str(&format!("  [{key}]"));
    }
    if !comment.is_empty() {
        out.push_str(&format!("  {comment}"));
    }
    out
}

/// 관계 표기를 왼쪽·관계·오른쪽으로 나눈다.
fn split_relation(t: &str) -> Option<(String, String, String)> {
    let chars: Vec<char> = t.chars().collect();
    // 관계 기호는 `|o` `||` `}o` `}|` + `--`/`..` + 그 거울상
    let mut i = 0;
    while i + 1 < chars.len() {
        if (chars[i] == '-' && chars[i + 1] == '-') || (chars[i] == '.' && chars[i + 1] == '.') {
            // 왼쪽으로 최대 2글자, 오른쪽으로 최대 2글자가 카디널리티다.
            let ls = i.saturating_sub(2);
            let mut lstart = i;
            while lstart > ls && matches!(chars[lstart - 1], '|' | 'o' | '{' | '}') {
                lstart -= 1;
            }
            let mut rend = i + 2;
            while rend < chars.len() && rend < i + 4 && matches!(chars[rend], '|' | 'o' | '{' | '}') {
                rend += 1;
            }
            if lstart == i && rend == i + 2 {
                i += 1;
                continue;
            }
            let left: String = chars[..lstart].iter().collect();
            let rel: String = chars[lstart..rend].iter().collect();
            let right: String = chars[rend..].iter().collect();
            return Some((left.trim().to_string(), rel, right.trim().to_string()));
        }
        i += 1;
    }
    None
}

/// 관계 기호에서 한쪽 카디널리티를 읽는다. `||--o{` → 왼쪽 `1`, 오른쪽 `0..N`.
fn cardinality(rel: &str, left: bool) -> String {
    let is_card = |c: char| matches!(c, '|' | 'o' | '{' | '}');
    let side: String = if left {
        rel.chars().take_while(|c| is_card(*c)).collect()
    } else {
        let tail: String = rel.chars().rev().take_while(|c| is_card(*c)).collect();
        tail.chars().rev().collect()
    };
    match side.as_str() {
        "||" => "1".into(),
        "|o" | "o|" => "0..1".into(),
        "}o" | "o{" => "0..N".into(),
        "}|" | "|{" => "1..N".into(),
        "" => String::new(),
        _ => "N".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relation_cardinalities() {
        let g = parse("erDiagram\n MEMBER ||--o{ ENROLLMENT : \"수강신청\"\n");
        assert_eq!(g.nodes.len(), 2);
        assert_eq!(g.edges[0].tail_tag, "1");
        assert_eq!(g.edges[0].head_tag, "0..N");
        assert_eq!(g.edges[0].label, "수강신청");
    }

    #[test]
    fn attributes_become_rows() {
        let g = parse("erDiagram\n MEMBER {\n  bigint member_id PK\n  varchar name\n }\n");
        assert_eq!(g.nodes[0].lines[0], "MEMBER");
        assert_eq!(g.nodes[0].lines[1], DIVIDER);
        assert_eq!(g.nodes[0].lines[2], "member_id : bigint  [PK]");
        assert_eq!(g.nodes[0].lines[3], "name : varchar");
    }

    #[test]
    fn attribute_comment_is_kept() {
        assert_eq!(attribute(r#"varchar role "LEARNER|ADMIN""#), "role : varchar  LEARNER|ADMIN");
    }

    #[test]
    fn optional_one_to_one() {
        let g = parse("erDiagram\n A ||--o| B : x\n");
        assert_eq!(g.edges[0].head_tag, "0..1");
    }
}
