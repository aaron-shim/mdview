//! HTML `<table>` 블록을 표로 그린다. `rowspan`·`colspan` 병합을 지원한다.

use crate::doc::{Line, Span};
use crate::render::canvas::{Canvas, width_of};
use crate::render::wrap_plain;
use crate::theme::Theme;

#[derive(Clone, Debug)]
struct Cell {
    text: String,
    colspan: usize,
    rowspan: usize,
    header: bool,
    /// 배치 뒤 확정되는 위치
    row: usize,
    col: usize,
}

/// HTML 조각이 표라면 그려서 돌려준다.
pub fn render(html: &str, theme: &Theme, width: usize) -> Option<Vec<Line>> {
    if !html.to_ascii_lowercase().contains("<table") {
        return None;
    }
    let (caption, cells, ncols, nrows) = parse(html)?;
    if cells.is_empty() || ncols == 0 {
        return None;
    }
    Some(draw(&caption, &cells, ncols, nrows, theme, width))
}

/// `<table>` 안의 칸을 읽어 격자 위치까지 정한다.
fn parse(html: &str) -> Option<(String, Vec<Cell>, usize, usize)> {
    let mut cells: Vec<Cell> = Vec::new();
    let mut caption = String::new();
    let chars: Vec<char> = html.chars().collect();
    let mut i = 0usize;
    // 각 행이 이미 차지한 칸: (행, 열) → 사용중
    let mut used: Vec<Vec<bool>> = Vec::new();
    let mut row: usize = 0;
    let mut started = false;

    while i < chars.len() {
        let Some((name, attrs, end)) = read_tag(&chars, i) else {
            i += 1;
            continue;
        };
        i = end;
        match name.as_str() {
            "tr" => {
                if started {
                    row += 1;
                }
                started = true;
            }
            "/table" => break,
            "caption" => {
                let (text, next) = read_until_close(&chars, i, "caption");
                caption = clean(&text);
                i = next;
            }
            "td" | "th" => {
                let (text, next) = read_until_close(&chars, i, &name);
                i = next;
                let colspan = attr(&attrs, "colspan").and_then(|v| v.parse().ok()).unwrap_or(1usize).max(1);
                let rowspan = attr(&attrs, "rowspan").and_then(|v| v.parse().ok()).unwrap_or(1usize).max(1);
                while used.len() <= row + rowspan {
                    used.push(Vec::new());
                }
                // 이 행에서 비어 있는 첫 열을 찾는다.
                let mut col = 0usize;
                loop {
                    if used[row].len() <= col {
                        used[row].resize(col + 1, false);
                    }
                    if !used[row][col] {
                        break;
                    }
                    col += 1;
                }
                for slots in used[row..row + rowspan].iter_mut() {
                    if slots.len() < col + colspan {
                        slots.resize(col + colspan, false);
                    }
                    for slot in &mut slots[col..col + colspan] {
                        *slot = true;
                    }
                }
                cells.push(Cell { text: clean(&text), colspan, rowspan, header: name == "th", row, col });
            }
            _ => {}
        }
    }
    let ncols = used.iter().map(Vec::len).max().unwrap_or(0);
    let nrows = cells.iter().map(|c| c.row + c.rowspan).max().unwrap_or(0);
    Some((caption, cells, ncols, nrows))
}

/// `<tag attr="v">` 를 읽는다. 반환값은 (태그 이름, 속성 문자열, 끝 위치).
fn read_tag(c: &[char], i: usize) -> Option<(String, String, usize)> {
    if c.get(i) != Some(&'<') {
        return None;
    }
    let mut j = i + 1;
    let mut name = String::new();
    if c.get(j) == Some(&'/') {
        name.push('/');
        j += 1;
    }
    while let Some(&ch) = c.get(j) {
        if ch.is_alphanumeric() {
            name.push(ch.to_ascii_lowercase());
            j += 1;
        } else {
            break;
        }
    }
    if name.is_empty() || name == "/" {
        return None;
    }
    let astart = j;
    let mut quote: Option<char> = None;
    while let Some(&ch) = c.get(j) {
        match quote {
            Some(q) => {
                if ch == q {
                    quote = None;
                }
            }
            None => {
                if ch == '"' || ch == '\'' {
                    quote = Some(ch);
                } else if ch == '>' {
                    break;
                }
            }
        }
        j += 1;
    }
    let attrs: String = c[astart..j.min(c.len())].iter().collect();
    Some((name, attrs, (j + 1).min(c.len())))
}

fn attr(attrs: &str, key: &str) -> Option<String> {
    let lower = attrs.to_ascii_lowercase();
    let p = lower.find(&format!("{key}="))?;
    let rest = attrs[p + key.len() + 1..].trim_start();
    let mut it = rest.chars();
    match it.next() {
        Some(q @ ('"' | '\'')) => Some(rest[1..].split(q).next()?.to_string()),
        _ => Some(rest.split_whitespace().next()?.to_string()),
    }
}

/// 닫는 태그 전까지의 내용을 읽는다.
fn read_until_close(c: &[char], i: usize, name: &str) -> (String, usize) {
    let close = format!("/{name}");
    let mut j = i;
    let start = i;
    while j < c.len() {
        if c[j] == '<'
            && let Some((n, _, end)) = read_tag(c, j)
        {
            if n == close {
                return (c[start..j].iter().collect(), end);
            }
            // 같은 이름의 다음 칸이 열리면(닫는 태그 생략) 거기서 끊는다.
            if n == name || n == "tr" || n == "/tr" || n == "/table" {
                return (c[start..j].iter().collect(), j);
            }
            j = end;
            continue;
        }
        j += 1;
    }
    (c[start..].iter().collect(), c.len())
}

/// 태그를 걷어내고 실체를 되돌린다.
fn clean(s: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '<'
            && let Some((n, _, end)) = read_tag(&chars, i) {
                if n == "br" || n == "/br" {
                    out.push('\n');
                }
                i = end;
                continue;
            }
        out.push(chars[i]);
        i += 1;
    }
    let out = out
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&");
    out.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n")
}

fn draw(caption: &str, cells: &[Cell], ncols: usize, nrows: usize, theme: &Theme, width: usize) -> Vec<Line> {
    // 열 폭: 한 칸짜리부터 정하고, 병합 칸이 모자라면 넓힌다.
    let mut colw = vec![3usize; ncols];
    for c in cells.iter().filter(|c| c.colspan == 1) {
        let w = c.text.lines().map(width_of).max().unwrap_or(0);
        colw[c.col] = colw[c.col].max(w.min(28));
    }
    for c in cells.iter().filter(|c| c.colspan > 1) {
        let need = c.text.lines().map(width_of).max().unwrap_or(0).min(40);
        let have: usize = (c.col..c.col + c.colspan).map(|i| colw[i] + 3).sum::<usize>() - 3;
        if have < need {
            let add = (need - have).div_ceil(c.colspan);
            for w in &mut colw[c.col..c.col + c.colspan] {
                *w += add;
            }
        }
    }
    // 폭 맞추기
    let total = |colw: &[usize]| colw.iter().map(|w| w + 3).sum::<usize>() + 1;
    while total(&colw) > width && colw.iter().any(|w| *w > 3) {
        let m = colw.iter().copied().max().unwrap_or(0);
        if let Some(i) = colw.iter().position(|w| *w == m) {
            colw[i] -= 1;
        }
    }
    let colx: Vec<usize> = (0..=ncols).map(|i| colw[..i].iter().map(|w| w + 3).sum::<usize>()).collect();

    // 칸마다 줄바꿈한 내용과 행 높이
    let wrapped: Vec<Vec<String>> = cells
        .iter()
        .map(|c| {
            let w = colx[c.col + c.colspan] - colx[c.col] - 3;
            c.text.lines().flat_map(|l| wrap_plain(l, w)).collect()
        })
        .collect();
    let mut rowh = vec![1usize; nrows];
    for (i, c) in cells.iter().enumerate() {
        if c.rowspan == 1 {
            rowh[c.row] = rowh[c.row].max(wrapped[i].len());
        }
    }
    for (i, c) in cells.iter().enumerate() {
        let have: usize = (c.row..c.row + c.rowspan).map(|r| rowh[r] + 1).sum::<usize>() - 1;
        if have < wrapped[i].len() {
            let add = (wrapped[i].len() - have).div_ceil(c.rowspan);
            for h in &mut rowh[c.row..c.row + c.rowspan] {
                *h += add;
            }
        }
    }
    let rowy: Vec<usize> = (0..=nrows).map(|i| rowh[..i].iter().map(|h| h + 1).sum::<usize>()).collect();

    let mut c2 = Canvas::new(colx[ncols] + 2, rowy[nrows] + 2);
    let border = theme.table_border;
    for (i, cell) in cells.iter().enumerate() {
        let x = colx[cell.col];
        let w = colx[cell.col + cell.colspan] - x + 1;
        let y = rowy[cell.row];
        let h = rowy[cell.row + cell.rowspan] - y + 1;
        c2.rect_join(x, y, w, h, border);
        let st = if cell.header { theme.table_header } else { theme.text };
        let inner = w.saturating_sub(4);
        let top = y + 1 + (h.saturating_sub(2 + wrapped[i].len())) / 2;
        for (k, text) in wrapped[i].iter().enumerate() {
            let off = if cell.header { (inner.saturating_sub(width_of(text))) / 2 } else { 0 };
            c2.text(x + 2 + off, top + k, text, st);
        }
    }
    let mut out = Vec::new();
    if !caption.is_empty() {
        out.push(Line::from_spans(vec![Span::new(caption.to_string(), theme.table_header)]));
    }
    out.extend(c2.into_lines());
    while out.last().is_some_and(|l| l.plain().trim().is_empty()) {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out(html: &str) -> Vec<String> {
        render(html, &Theme::notty(), 80).unwrap().iter().map(Line::plain).collect()
    }

    #[test]
    fn simple_table() {
        let o = out("<table><tr><th>a</th><th>b</th></tr><tr><td>1</td><td>2</td></tr></table>");
        assert_eq!(o[0], "┌─────┬─────┐");
        assert!(o[1].contains('a') && o[1].contains('b'));
        assert_eq!(o[2], "├─────┼─────┤");
        assert_eq!(o.last().unwrap(), "└─────┴─────┘");
    }

    #[test]
    fn colspan_merges_columns() {
        let o = out("<table><tr><th colspan=\"2\">묶음</th></tr><tr><td>1</td><td>2</td></tr></table>");
        // 첫 줄은 가운데 칸막이가 없어야 한다.
        assert!(!o[0].contains('┬'));
        assert!(o[2].contains('┬'), "{o:?}");
    }

    #[test]
    fn rowspan_merges_rows() {
        let o = out("<table><tr><td rowspan=\"2\">긴칸</td><td>1</td></tr><tr><td>2</td></tr></table>");
        let mid = o.iter().find(|l| l.contains('┼') || l.contains('├')).unwrap();
        assert!(mid.starts_with('│'), "rowspan 칸의 왼쪽은 이어져야 한다: {mid}");
    }

    #[test]
    fn not_a_table_returns_none() {
        assert!(render("<div>hi</div>", &Theme::notty(), 80).is_none());
    }

    #[test]
    fn br_breaks_lines_in_cell() {
        let o = out("<table><tr><td>위<br>아래</td></tr></table>");
        assert!(o.iter().any(|l| l.contains("위")));
        assert!(o.iter().any(|l| l.contains("아래")));
    }
}
