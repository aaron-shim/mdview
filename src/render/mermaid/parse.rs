//! mermaid 원문을 다루는 공통 도우미.

/// 주석·지시자를 걷어내고 의미 있는 줄만 남긴다.
pub fn clean_lines(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in src.lines() {
        let mut line = raw.to_string();
        // `%%{init: ...}%%` 지시자와 `%%` 주석 제거
        if let Some(p) = find_comment(&line) {
            line.truncate(p);
        }
        let t = line.trim_end().to_string();
        if t.trim().is_empty() {
            continue;
        }
        out.push(t);
    }
    out
}

/// 따옴표 밖에 있는 `%%`의 위치.
fn find_comment(s: &str) -> Option<usize> {
    let b: Vec<char> = s.chars().collect();
    let mut quote = None::<char>;
    let mut idx = 0usize;
    for i in 0..b.len() {
        let c = b[i];
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                }
            }
            None => {
                if c == '"' || c == '\'' {
                    quote = Some(c);
                } else if c == '%' && b.get(i + 1) == Some(&'%') {
                    return Some(idx);
                }
            }
        }
        idx += c.len_utf8();
    }
    None
}

/// 라벨 정리: 따옴표 제거, `<br>`를 줄바꿈으로, HTML 실체 해제.
pub fn label(s: &str) -> String {
    let mut t = s.trim().to_string();
    for q in ['"', '\''] {
        if t.len() >= 2 && t.starts_with(q) && t.ends_with(q) {
            t = t[q.len_utf8()..t.len() - q.len_utf8()].to_string();
        }
    }
    let t = t
        .replace("<br/>", "\n")
        .replace("<br />", "\n")
        .replace("<br>", "\n")
        .replace("\\n", "\n")
        .replace("#quot;", "\"")
        .replace("&quot;", "\"")
        .replace("#35;", "#")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ");
    t.lines().map(str::trim).collect::<Vec<_>>().join("\n")
}

/// 첫 낱말(지시어)을 돌려준다.
pub fn keyword(s: &str) -> &str {
    s.trim().split(|c: char| c.is_whitespace()).next().unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_comments_and_blank_lines() {
        let out = clean_lines("graph TD %% 주석\n\n  A --> B\n%% 전체 주석\n");
        assert_eq!(out, vec!["graph TD", "  A --> B"]);
    }

    #[test]
    fn keeps_percent_inside_quotes() {
        let out = clean_lines(r#"A["99%% 가용성"]"#);
        assert_eq!(out, vec![r#"A["99%% 가용성"]"#]);
    }

    #[test]
    fn label_unwraps_quotes_and_breaks() {
        assert_eq!(label(r#""api-core<br/>Java 8""#), "api-core\nJava 8");
        assert_eq!(label("plain"), "plain");
    }
}
