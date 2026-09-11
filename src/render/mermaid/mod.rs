//! mermaid 다이어그램을 터미널 그림으로 그린다.
//!
//! 지원: flowchart/graph · sequenceDiagram · stateDiagram · erDiagram ·
//! classDiagram · gantt · pie · gitGraph. 그 밖의 종류는 `None`을 돌려주어
//! 호출자가 원문 코드블록으로 보여주게 한다.
//!
//! 터미널은 세로로 길고 가로로 좁기 때문에, 그래프 계열은 방향 지정과
//! 무관하게 위에서 아래로 배치한다.

pub mod class;
pub mod er;
pub mod flow;
pub mod gantt;
pub mod git;
pub mod graph;
pub mod parse;
pub mod pie;
pub mod sequence;
pub mod state;

use crate::doc::{Line, Span};
use crate::theme::Theme;

/// 다이어그램 종류 이름(캡션에 쓴다).
fn kind_of(src: &str) -> Option<&'static str> {
    for line in parse::clean_lines(src) {
        let t = line.trim();
        if t.is_empty() || t.starts_with("---") {
            continue;
        }
        let kw = parse::keyword(t).to_ascii_lowercase();
        let kw = kw.trim_end_matches(|c: char| !c.is_alphanumeric() && c != '-').to_string();
        return Some(match kw.as_str() {
            "flowchart" | "flowchart-v2" | "graph" => "flowchart",
            "sequencediagram" => "sequence",
            "statediagram" | "statediagram-v2" => "state",
            "erdiagram" => "er",
            "classdiagram" | "classdiagram-v2" => "class",
            "gantt" => "gantt",
            "pie" => "pie",
            "gitgraph" => "git",
            _ => return None,
        });
    }
    None
}

/// mermaid 코드를 그림 줄들로 바꾼다. 지원하지 않으면 `None`.
pub fn render(src: &str, theme: &Theme, width: usize) -> Option<Vec<Line>> {
    let kind = kind_of(src)?;
    let body = match kind {
        "flowchart" => flow::parse(src).render(theme, width),
        "state" => state::parse(src).render(theme, width),
        "er" => er::parse(src).render(theme, width),
        "class" => class::parse(src).render(theme, width),
        "sequence" => sequence::render(src, theme, width),
        "gantt" => gantt::render(src, theme, width),
        "pie" => pie::render(src, theme, width),
        "git" => git::render(src, theme, width),
        _ => return None,
    };
    if body.iter().all(Line::is_empty) {
        return None;
    }
    // 터미널 폭을 넘기면 잘려서 못 알아보므로, 원문 코드블록으로 넘긴다.
    if body.iter().map(Line::width).max().unwrap_or(0) > width {
        return None;
    }
    let mut out = vec![caption(kind, theme, width)];
    out.extend(body);
    // 위아래 빈 줄 정리
    while out.last().is_some_and(|l| l.plain().trim().is_empty()) {
        out.pop();
    }
    Some(out)
}

/// 다이어그램 머리말.
fn caption(kind: &str, theme: &Theme, width: usize) -> Line {
    let text = format!("◈ mermaid · {kind} ");
    let pad = width.saturating_sub(crate::render::canvas::width_of(&text)).min(40);
    Line::from_spans(vec![Span::new(text, theme.diagram_title), Span::new("─".repeat(pad), theme.diagram_border)])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_kinds() {
        assert_eq!(kind_of("flowchart TB\nA-->B"), Some("flowchart"));
        assert_eq!(kind_of("  sequenceDiagram\n A->>B: hi"), Some("sequence"));
        assert_eq!(kind_of("stateDiagram-v2\n[*]-->A"), Some("state"));
        assert_eq!(kind_of("mindmap\n root"), None);
    }

    #[test]
    fn unsupported_returns_none() {
        assert!(render("mindmap\n  root((x))", &Theme::notty(), 80).is_none());
    }
}

#[cfg(test)]
mod robustness {
    use super::*;

    /// 잘리거나 이상한 입력에도 패닉 없이 무언가를 돌려주거나 `None`이어야 한다.
    #[test]
    fn odd_inputs_do_not_panic() {
        let cases = [
            "flowchart TB",
            "flowchart TB\n A",
            "flowchart LR\n A --> A",
            "flowchart TB\n A --> B\n B --> A\n",
            "flowchart TB\n subgraph S\n end\n A --> B",
            "flowchart TB\n A[\"unclosed",
            "flowchart TB\n A -- --> B",
            "graph TD\n A-->B-->C-->A",
            "sequenceDiagram",
            "sequenceDiagram\n A->>A: 자기호출\n alt\n end\n",
            "sequenceDiagram\n Note over A: 홀로",
            "stateDiagram-v2\n [*] --> [*]",
            "stateDiagram-v2\n state X {\n }\n",
            "erDiagram\n A {\n }\n",
            "erDiagram\n A ||--|| B",
            "classDiagram\n class A\n",
            "classDiagram\n A <|-- B : \n",
            "gantt\n section S\n A :\n",
            "gantt\n A :x, 2026-13-45, 0d\n",
            "pie\n title 없음\n",
            "pie\n \"A\" : 0\n",
            "gitGraph\n merge nowhere\n",
            "gitGraph\n checkout nope\n commit\n",
        ];
        for src in cases {
            for w in [20usize, 40, 80, 200] {
                let _ = render(src, &Theme::notty(), w);
                let _ = render(src, &Theme::dark(), w);
            }
        }
    }

    #[test]
    fn very_narrow_width_still_renders() {
        let out = render("flowchart TB\n A[\"아주 긴 이름을 가진 노드\"] --> B[\"또 다른 긴 이름\"]\n", &Theme::notty(), 24);
        assert!(out.is_some());
    }
}
