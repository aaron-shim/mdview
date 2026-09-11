//! 수식 조판을 위한 2차원 상자 모델.
//!
//! 각 상자는 문자 줄 목록과 기준선(baseline) 행을 갖는다. 가로로 이을 때
//! 기준선을 맞추므로 분수·행렬·큰 연산자가 자연스럽게 정렬된다.

use crate::render::canvas::width_of;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MBox {
    pub lines: Vec<String>,
    pub width: usize,
    /// 기준선이 되는 줄 번호.
    pub base: usize,
}

impl MBox {
    pub fn empty() -> Self {
        MBox { lines: vec![String::new()], width: 0, base: 0 }
    }

    /// 한 줄짜리 상자.
    pub fn text(s: impl Into<String>) -> Self {
        let s = s.into();
        let width = width_of(&s);
        MBox { lines: vec![s], width, base: 0 }
    }

    pub fn height(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.width == 0 && self.height() <= 1
    }

    /// 기준선 위쪽 줄 수.
    fn above(&self) -> usize {
        self.base
    }

    /// 기준선 아래쪽 줄 수(기준선 포함).
    fn below(&self) -> usize {
        self.height() - self.base
    }

    fn pad_to(&self, w: usize) -> Vec<String> {
        self.lines.iter().map(|l| pad_right(l, w)).collect()
    }

    /// 모든 줄을 폭에 맞춰 가운데 정렬한다.
    pub fn center(&self, w: usize) -> Vec<String> {
        self.lines
            .iter()
            .map(|l| {
                let lw = width_of(l);
                let pad = w.saturating_sub(lw);
                let left = pad / 2;
                format!("{}{}{}", " ".repeat(left), l, " ".repeat(pad - left))
            })
            .collect()
    }

    /// 두 상자를 기준선을 맞춰 가로로 잇는다.
    pub fn hcat(self, other: MBox) -> MBox {
        if self.is_empty() {
            return other;
        }
        if other.is_empty() {
            return self;
        }
        let above = self.above().max(other.above());
        let below = self.below().max(other.below());
        let width = self.width + other.width;
        let a = self.pad_to(self.width);
        let b = other.pad_to(other.width);
        let mut lines = Vec::with_capacity(above + below);
        for i in 0..above + below {
            let ai = (i + self.above()).checked_sub(above).and_then(|k| a.get(k));
            let bi = (i + other.above()).checked_sub(above).and_then(|k| b.get(k));
            let mut s = String::new();
            s.push_str(ai.map(String::as_str).unwrap_or(&" ".repeat(self.width)));
            s.push_str(bi.map(String::as_str).unwrap_or(&" ".repeat(other.width)));
            lines.push(s);
        }
        MBox { lines, width, base: above }
    }

    /// 여러 상자를 차례로 잇는다.
    pub fn row(boxes: Vec<MBox>) -> MBox {
        boxes.into_iter().fold(MBox::empty(), MBox::hcat)
    }

    /// 분수: 분자 / 가로줄 / 분모.
    pub fn frac(num: MBox, den: MBox) -> MBox {
        let inner = num.width.max(den.width);
        let width = inner + 2;
        let mut lines = num.center(width);
        let base = lines.len();
        lines.push("─".repeat(width));
        lines.extend(den.center(width));
        MBox { lines, width, base }
    }

    /// 위/아래에 한계를 붙인 큰 연산자.
    pub fn limits(op: MBox, upper: Option<MBox>, lower: Option<MBox>) -> MBox {
        let width = op.width.max(upper.as_ref().map_or(0, |b| b.width)).max(lower.as_ref().map_or(0, |b| b.width));
        let mut lines = Vec::new();
        if let Some(u) = &upper {
            lines.extend(u.center(width));
        }
        let base = lines.len() + op.base;
        lines.extend(op.center(width));
        if let Some(l) = &lower {
            lines.extend(l.center(width));
        }
        MBox { lines, width, base }
    }

    /// 제곱근.
    pub fn sqrt(inner: MBox) -> MBox {
        let width = inner.width + 3;
        let mut lines = Vec::with_capacity(inner.height() + 1);
        lines.push(format!("  {}", "─".repeat(inner.width + 1)));
        for (i, l) in inner.pad_to(inner.width).into_iter().enumerate() {
            let head = if i == inner.base { "√ " } else { "  " };
            lines.push(format!("{head}{l} "));
        }
        MBox { lines, width, base: inner.base + 1 }
    }

    /// 내용 위에 덧줄(overline) 또는 강세 기호를 얹는다.
    pub fn accent(inner: MBox, mark: char) -> MBox {
        let width = inner.width;
        let top = if mark == '─' {
            "─".repeat(width)
        } else {
            let pad = width.saturating_sub(1);
            let left = pad / 2;
            format!("{}{}{}", " ".repeat(left), mark, " ".repeat(pad - left))
        };
        let mut lines = vec![top];
        lines.extend(inner.pad_to(width));
        MBox { lines, width, base: inner.base + 1 }
    }

    /// 상자를 늘어난 괄호로 감싼다.
    pub fn fence(inner: MBox, left: &str, right: &str) -> MBox {
        let h = inner.height();
        let lcol = fence_column(left, h);
        let rcol = fence_column(right, h);
        let lw = lcol.first().map_or(0, |s| width_of(s));
        let rw = rcol.first().map_or(0, |s| width_of(s));
        let width = inner.width + lw + rw;
        let body = inner.pad_to(inner.width);
        let lines = body
            .into_iter()
            .enumerate()
            .map(|(i, l)| format!("{}{}{}", lcol.get(i).map(String::as_str).unwrap_or(""), l, rcol.get(i).map(String::as_str).unwrap_or("")))
            .collect();
        MBox { lines, width, base: inner.base }
    }

    /// 행렬·정렬 격자. `align`은 열별 정렬(`l`, `c`, `r`).
    pub fn grid(rows: Vec<Vec<MBox>>, aligns: &[char], gap: usize) -> MBox {
        let ncol = rows.iter().map(Vec::len).max().unwrap_or(0);
        if ncol == 0 {
            return MBox::empty();
        }
        let mut colw = vec![0usize; ncol];
        for r in &rows {
            for (i, c) in r.iter().enumerate() {
                colw[i] = colw[i].max(c.width);
            }
        }
        let mut lines: Vec<String> = Vec::new();
        let mut base = 0;
        for (ri, row) in rows.iter().enumerate() {
            // 행 안에서 기준선을 맞춘다.
            let above = row.iter().map(MBox::above).max().unwrap_or(0);
            let below = row.iter().map(MBox::below).max().unwrap_or(1);
            let start = lines.len();
            for _ in 0..above + below {
                lines.push(String::new());
            }
            for (ci, cell) in row.iter().enumerate() {
                let off = above - cell.above();
                let al = aligns.get(ci).copied().unwrap_or('c');
                let padded: Vec<String> = match al {
                    'l' => cell.pad_to(colw[ci]),
                    'r' => cell.lines.iter().map(|l| format!("{}{}", " ".repeat(colw[ci] - width_of(l)), l)).collect(),
                    _ => cell.center(colw[ci]),
                };
                for (i, text) in padded.into_iter().enumerate() {
                    let line = &mut lines[start + off + i];
                    let want = col_offset(&colw, ci, gap);
                    let cur = width_of(line);
                    if cur < want {
                        line.push_str(&" ".repeat(want - cur));
                    }
                    line.push_str(&text);
                }
            }
            if ri == rows.len() / 2 {
                base = start + above;
            }
        }
        let width = col_offset(&colw, ncol, gap).saturating_sub(gap);
        let lines = lines.into_iter().map(|l| pad_right(&l, width)).collect();
        MBox { lines, width, base }
    }
}

fn col_offset(colw: &[usize], col: usize, gap: usize) -> usize {
    colw.iter().take(col).map(|w| w + gap).sum()
}

fn pad_right(s: &str, w: usize) -> String {
    let cur = width_of(s);
    if cur >= w { s.to_string() } else { format!("{}{}", s, " ".repeat(w - cur)) }
}

/// 높이에 맞춘 괄호 세로 조각들.
fn fence_column(kind: &str, h: usize) -> Vec<String> {
    if kind.is_empty() {
        return vec![String::new(); h];
    }
    if h == 1 {
        return vec![kind.to_string()];
    }
    let (top, mid, bot, ext) = match kind {
        "(" => ("⎛", "⎜", "⎝", "⎜"),
        ")" => ("⎞", "⎟", "⎠", "⎟"),
        "[" => ("⎡", "⎢", "⎣", "⎢"),
        "]" => ("⎤", "⎥", "⎦", "⎥"),
        "{" => ("⎧", "⎨", "⎩", "⎪"),
        "}" => ("⎫", "⎬", "⎭", "⎪"),
        "|" => ("│", "│", "│", "│"),
        "‖" => ("║", "║", "║", "║"),
        "⌊" => ("⎢", "⎢", "⎣", "⎢"),
        "⌋" => ("⎥", "⎥", "⎦", "⎥"),
        other => (other, other, other, other),
    };
    let mut out = Vec::with_capacity(h);
    let center = (h - 1) / 2;
    for i in 0..h {
        let s = if i == 0 {
            top
        } else if i + 1 == h {
            bot
        } else if (kind == "{" || kind == "}") && i == center {
            mid
        } else {
            ext
        };
        out.push(s.to_string());
    }
    out
}

impl MBox {
    /// 본체 오른쪽에 위·아래 첨자를 붙인다(유니코드 첨자를 못 쓸 때).
    pub fn scripts(nucleus: MBox, sup: Option<MBox>, sub: Option<MBox>) -> MBox {
        if sup.is_none() && sub.is_none() {
            return nucleus;
        }
        let w = sup.as_ref().map_or(0, |b| b.width).max(sub.as_ref().map_or(0, |b| b.width));
        let mut lines: Vec<String> = Vec::new();
        if let Some(s) = &sup {
            lines.extend(s.pad_to(w));
        }
        let base = lines.len() + nucleus.base;
        for _ in 0..nucleus.height() {
            lines.push(" ".repeat(w));
        }
        if let Some(s) = &sub {
            lines.extend(s.pad_to(w));
        }
        let col = MBox { lines, width: w, base };
        nucleus.hcat(col)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hcat_aligns_baselines() {
        let a = MBox::frac(MBox::text("1"), MBox::text("2"));
        let b = MBox::text("x");
        let r = a.hcat(MBox::text(" + ")).hcat(b);
        assert_eq!(r.lines, vec![" 1     ", "─── + x", " 2     "]);
        assert_eq!(r.base, 1);
    }

    #[test]
    fn frac_centers_and_draws_bar() {
        let f = MBox::frac(MBox::text("a"), MBox::text("bcd"));
        assert_eq!(f.lines, vec!["  a  ", "─────", " bcd "]);
        assert_eq!(f.base, 1);
    }

    #[test]
    fn limits_stack_above_and_below() {
        let b = MBox::limits(MBox::text("∑"), Some(MBox::text("n")), Some(MBox::text("i=1")));
        assert_eq!(b.lines, vec![" n ", " ∑ ", "i=1"]);
        assert_eq!(b.base, 1);
    }

    #[test]
    fn grid_aligns_columns() {
        let rows = vec![
            vec![MBox::text("a"), MBox::text("bb")],
            vec![MBox::text("ccc"), MBox::text("d")],
        ];
        let g = MBox::grid(rows, &['l', 'l'], 1);
        assert_eq!(g.lines, vec!["a   bb", "ccc d "]);
    }

    #[test]
    fn fence_wraps_tall_box() {
        let inner = MBox::grid(vec![vec![MBox::text("1")], vec![MBox::text("2")]], &['c'], 1);
        let f = MBox::fence(inner, "[", "]");
        assert_eq!(f.lines, vec!["⎡1⎤", "⎣2⎦"]);
    }

    #[test]
    fn sqrt_draws_overbar() {
        let s = MBox::sqrt(MBox::text("x"));
        assert_eq!(s.lines, vec!["  ──", "√ x "]);
    }
}
