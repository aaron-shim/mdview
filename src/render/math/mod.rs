//! LaTeX 수식을 터미널 문자로 조판한다.
//!
//! KaTeX 전체를 흉내내지는 않고, 문서에서 실제로 자주 쓰는 범위
//! (분수·근호·첨자·행렬·케이스·큰 연산자·강세·그리스 문자)를 다룬다.
//! 알 수 없는 명령은 이름을 그대로 남겨 원문을 잃지 않는다.

pub mod layout;
pub mod symbols;

use crate::doc::{Line, Span};
use crate::theme::Theme;
use layout::MBox;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// 문단 안에 섞여 들어가므로 한 줄로 유지한다.
    Inline,
    /// 독립된 블록. 2차원 조판을 쓴다.
    Display,
}

/// 인라인 수식 → 한 줄 문자열.
pub fn render_inline(latex: &str) -> String {
    let b = build(latex, Mode::Inline);
    if b.height() == 1 {
        return b.lines[0].trim_end().to_string();
    }
    // 인라인인데도 줄이 늘어나면 납작하게 눌러 붙인다.
    b.lines.iter().map(|l| l.trim()).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" ")
}

/// 블록 수식 → 폭 안에 가운데 정렬된 줄들.
///
/// 2차원 조판이 폭을 넘치면 한 줄짜리 표기로 낮추고, 그래도 넘치면 줄바꿈한다.
pub fn render_block(latex: &str, theme: &Theme, width: usize) -> Vec<Line> {
    let st = theme.math;
    let mut b = build(latex, Mode::Display);
    if b.width > width {
        b = build(latex, Mode::Inline);
    }
    if b.width > width {
        // 마지막 수단: 평문처럼 줄을 접는다.
        return b
            .lines
            .iter()
            .flat_map(|l| crate::render::wrap_plain(l.trim(), width))
            .map(|l| Line::from_spans(vec![Span::new(l, st)]))
            .collect();
    }
    // 줄마다 따로 맞추면 상자가 어긋나므로, 전체 폭 기준으로 한 번만 민다.
    let boxw = b.lines.iter().map(|l| crate::render::canvas::width_of(l.trim_end())).max().unwrap_or(0);
    let pad = " ".repeat(width.saturating_sub(boxw) / 2);
    b.lines
        .iter()
        .map(|l| Line::from_spans(vec![Span::new(format!("{}{}", pad, l.trim_end()), st)]))
        .collect()
}

/// 수식을 상자로 조판한다.
pub fn build(latex: &str, mode: Mode) -> MBox {
    let toks = tokenize(latex);
    let mut p = Parser { toks, pos: 0, mode, tight: 0 };
    let rows = p.parse_rows(&[]);
    if rows.len() == 1 && rows[0].len() == 1 {
        return rows[0][0].clone();
    }
    if mode == Mode::Inline {
        let joined: Vec<MBox> = rows
            .into_iter()
            .map(|r| MBox::row(r.into_iter().flat_map(|c| [c, MBox::text(" ")]).collect()))
            .collect();
        return MBox::row(joined);
    }
    let ncol = rows.iter().map(Vec::len).max().unwrap_or(1);
    let aligns: Vec<char> = (0..ncol).map(|_| 'c').collect();
    MBox::grid(rows, &aligns, 2)
}

// ---------------------------------------------------------------- 토큰

#[derive(Clone, Debug, PartialEq, Eq)]
enum Tok {
    Cmd(String),
    Char(char),
    Open,
    Close,
    Sup,
    Sub,
    Amp,
    NewRow,
    Space,
}

fn tokenize(s: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '\\' => match it.peek().copied() {
                Some('\\') => {
                    it.next();
                    // `\\[2pt]` 같은 선택 인자는 버린다.
                    if it.peek() == Some(&'[') {
                        for c2 in it.by_ref() {
                            if c2 == ']' {
                                break;
                            }
                        }
                    }
                    out.push(Tok::NewRow);
                }
                Some(c2) if c2.is_ascii_alphabetic() => {
                    let mut name = String::new();
                    while let Some(c3) = it.peek().copied() {
                        if c3.is_ascii_alphabetic() {
                            name.push(c3);
                            it.next();
                        } else {
                            break;
                        }
                    }
                    out.push(Tok::Cmd(name));
                }
                Some(c2) => {
                    it.next();
                    out.push(Tok::Cmd(c2.to_string()));
                }
                None => {}
            },
            '{' => out.push(Tok::Open),
            '}' => out.push(Tok::Close),
            '^' => out.push(Tok::Sup),
            '_' => out.push(Tok::Sub),
            '&' => out.push(Tok::Amp),
            '%' => {
                for c2 in it.by_ref() {
                    if c2 == '\n' {
                        break;
                    }
                }
            }
            c if c.is_whitespace() => out.push(Tok::Space),
            c => out.push(Tok::Char(c)),
        }
    }
    out
}

// ---------------------------------------------------------------- 원자 종류

/// 항목 사이 간격을 정하기 위한 분류.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Ord,
    Bin,
    Rel,
    Punct,
    Open,
    Close,
    /// 큰 연산자·함수 이름. 피연산자와 한 칸 띄운다.
    Op,
    /// 앞뒤로 간격을 두지 않는 조각(첨자·공백 등).
    Tight,
}

fn classify(s: &str) -> Kind {
    match s {
        "+" | "-" | "−" | "±" | "∓" | "×" | "÷" | "·" | "∗" | "⋆" | "∘" | "∪" | "∩" | "⊔" | "⊓" | "∧" | "∨" | "⊕" | "⊖" | "⊗" | "⊙" | "∖" => Kind::Bin,
        "=" | "<" | ">" | "≤" | "≥" | "≠" | "≡" | "∼" | "≃" | "≈" | "≅" | "∝" | "≍" | "≪" | "≫" | "≺" | "≻" | "⊂" | "⊃" | "⊆" | "⊇" | "∈" | "∋" | "∉" | "⊥" | "∥" | "⊨" | "⊢" | "⊣" | "→" | "←" | "↔" | "⇒" | "⇐" | "⇔" | "↦" | "⟶" | "⟵" | "⟹" | "⟺" | "↪" | "⇌" | ":=" => Kind::Rel,
        "," | ";" | ":" => Kind::Punct,
        "(" | "[" | "⟨" | "⌈" | "⌊" => Kind::Open,
        ")" | "]" | "⟩" | "⌉" | "⌋" | "!" => Kind::Close,
        _ => Kind::Ord,
    }
}

// ---------------------------------------------------------------- 파서

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
    mode: Mode,
    /// 첨자·한계 안이면 0보다 크다(간격을 좁힌다).
    tight: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn bump(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn skip_space(&mut self) {
        while matches!(self.peek(), Some(Tok::Space)) {
            self.pos += 1;
        }
    }

    /// 행(`\\`)과 열(`&`)로 나뉜 격자를 읽는다.
    fn parse_rows(&mut self, aligns: &[char]) -> Vec<Vec<MBox>> {
        let mut rows: Vec<Vec<MBox>> = Vec::new();
        let mut row: Vec<MBox> = Vec::new();
        loop {
            let cell = self.parse_seq();
            row.push(cell);
            match self.peek() {
                Some(Tok::Amp) => {
                    self.pos += 1;
                }
                Some(Tok::NewRow) => {
                    self.pos += 1;
                    rows.push(std::mem::take(&mut row));
                }
                _ => break,
            }
        }
        if row.iter().any(|b| !b.is_empty()) || rows.is_empty() {
            rows.push(row);
        }
        let _ = aligns;
        rows
    }

    /// 멈춤 토큰(`}`, `&`, `\\`, `\end`, `\right`) 전까지 읽는다.
    fn parse_seq(&mut self) -> MBox {
        let mut items: Vec<(MBox, Kind)> = Vec::new();
        loop {
            match self.peek() {
                None | Some(Tok::Close) | Some(Tok::Amp) | Some(Tok::NewRow) => break,
                Some(Tok::Cmd(c)) if c == "end" || c == "right" => break,
                _ => {}
            }
            let Some(item) = self.parse_item() else { break };
            items.push(item);
        }
        self.join(items)
    }

    /// 원자 하나 + 뒤따르는 첨자.
    fn parse_item(&mut self) -> Option<(MBox, Kind)> {
        self.skip_space();
        match self.peek() {
            None | Some(Tok::Close) | Some(Tok::Amp) | Some(Tok::NewRow) => return None,
            Some(Tok::Cmd(c)) if c == "end" || c == "right" => return None,
            _ => {}
        }
        let (nucleus, kind, limits) = self.parse_nucleus()?;
        let b = self.parse_scripts(nucleus, limits);
        Some((b, kind))
    }

    /// 첨자(`^`, `_`)를 모아 붙인다.
    fn parse_scripts(&mut self, nucleus: MBox, limits: bool) -> MBox {
        let mut sup: Option<MBox> = None;
        let mut sub: Option<MBox> = None;
        loop {
            match self.peek() {
                Some(Tok::Sup) => {
                    self.pos += 1;
                    self.tight += 1;
                    sup = Some(self.parse_group());
                    self.tight -= 1;
                }
                Some(Tok::Sub) => {
                    self.pos += 1;
                    self.tight += 1;
                    sub = Some(self.parse_group());
                    self.tight -= 1;
                }
                _ => break,
            }
        }
        if sup.is_none() && sub.is_none() {
            return nucleus;
        }
        if limits && self.mode == Mode::Display {
            return MBox::limits(nucleus, sup, sub);
        }
        // 1) 짧은 첨자는 유니코드 첨자 글자로 붙인다: x_i → xᵢ, 2^{10} → 2¹⁰
        let u_sup = sup.as_ref().and_then(|b| unicode_script(b, true));
        let u_sub = sub.as_ref().and_then(|b| unicode_script(b, false));
        if sup.is_some() == u_sup.is_some() && sub.is_some() == u_sub.is_some() {
            let text = format!("{}{}", u_sub.unwrap_or_default(), u_sup.unwrap_or_default());
            return nucleus.hcat(MBox::text(text));
        }
        // 2) 낱말 첨자는 밑줄 표기가 읽기 좋다: T_{total} → T_total
        let w_sup = sup.as_ref().and_then(plain_word);
        let w_sub = sub.as_ref().and_then(plain_word);
        if sup.is_some() == w_sup.is_some() && sub.is_some() == w_sub.is_some() {
            let mut text = String::new();
            if let Some(w) = w_sub {
                text.push('_');
                text.push_str(&w);
            }
            if let Some(w) = w_sup {
                text.push('^');
                text.push_str(&w);
            }
            return nucleus.hcat(MBox::text(text));
        }
        if self.mode == Mode::Inline {
            let mut out = nucleus;
            if let Some(b) = sub {
                out = out.hcat(MBox::text(format!("_({})", flatten(&b))));
            }
            if let Some(b) = sup {
                out = out.hcat(MBox::text(format!("^({})", flatten(&b))));
            }
            return out;
        }
        MBox::scripts(nucleus, sup, sub)
    }

    /// `{...}` 또는 원자 하나.
    fn parse_group(&mut self) -> MBox {
        self.skip_space();
        if matches!(self.peek(), Some(Tok::Open)) {
            self.pos += 1;
            let b = self.parse_seq();
            if matches!(self.peek(), Some(Tok::Close)) {
                self.pos += 1;
            }
            return b;
        }
        match self.parse_item() {
            Some((b, _)) => b,
            None => MBox::empty(),
        }
    }

    /// `{...}` 안의 글자를 공백까지 그대로 읽는다(`\text` 계열).
    fn parse_raw_group(&mut self) -> String {
        self.skip_space();
        let mut out = String::new();
        if !matches!(self.peek(), Some(Tok::Open)) {
            if let Some(t) = self.bump() {
                out.push_str(&tok_text(&t));
            }
            return out;
        }
        self.pos += 1;
        let mut depth = 1;
        while let Some(t) = self.bump() {
            match &t {
                Tok::Open => {
                    depth += 1;
                    out.push('{');
                }
                Tok::Close => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    out.push('}');
                }
                other => out.push_str(&tok_text(other)),
            }
        }
        out
    }

    /// 본체 하나를 읽는다. 반환값의 셋째 항목은 "한계를 위아래로 붙이는지".
    fn parse_nucleus(&mut self) -> Option<(MBox, Kind, bool)> {
        let t = self.bump()?;
        Some(match t {
            Tok::Space => (MBox::empty(), Kind::Tight, false),
            Tok::Open => {
                let b = self.parse_seq();
                if matches!(self.peek(), Some(Tok::Close)) {
                    self.pos += 1;
                }
                (b, Kind::Ord, false)
            }
            Tok::Sup | Tok::Sub => {
                // 앞에 본체가 없는 첨자 → 빈 본체에 붙인다.
                self.pos -= 1;
                (MBox::empty(), Kind::Tight, false)
            }
            Tok::Char(c) => (MBox::text(c.to_string()), classify(&c.to_string()), false),
            Tok::Close | Tok::Amp | Tok::NewRow => (MBox::empty(), Kind::Tight, false),
            Tok::Cmd(name) => self.command(&name),
        })
    }

    fn command(&mut self, name: &str) -> (MBox, Kind, bool) {
        // 공백 명령
        if let Some(n) = symbols::spacing(name) {
            return (MBox::text(" ".repeat(n)), Kind::Tight, false);
        }
        match name {
            "frac" | "dfrac" | "tfrac" | "cfrac" => {
                let num = self.parse_group();
                let den = self.parse_group();
                return (self.make_frac(num, den), Kind::Ord, false);
            }
            "binom" | "dbinom" => {
                let a = self.parse_group();
                let b = self.parse_group();
                let inner = MBox::grid(vec![vec![a], vec![b]], &['c'], 0);
                return (MBox::fence(inner, "(", ")"), Kind::Ord, false);
            }
            "sqrt" => {
                self.skip_space();
                let mut index = None;
                if matches!(self.peek(), Some(Tok::Char('['))) {
                    self.pos += 1;
                    let mut s = String::new();
                    while let Some(t) = self.bump() {
                        if t == Tok::Char(']') {
                            break;
                        }
                        s.push_str(&tok_text(&t));
                    }
                    index = Some(s);
                }
                let inner = self.parse_group();
                let b = MBox::sqrt(inner);
                let b = match index {
                    Some(i) if !i.is_empty() => MBox::text(symbols::to_superscript(&i).unwrap_or(i)).hcat(b),
                    _ => b,
                };
                return (b, Kind::Ord, false);
            }
            "text" | "textrm" | "textbf" | "textit" | "mathrm" | "operatorname" | "mathsf" | "mathtt" | "mbox" | "textsf" | "textnormal" => {
                let s = self.parse_raw_group();
                return (MBox::text(s), Kind::Ord, false);
            }
            "mathbf" | "boldsymbol" | "bm" => {
                let s = self.parse_raw_group();
                return (MBox::text(s.chars().map(symbols::math_bold).collect::<String>()), Kind::Ord, false);
            }
            "mathbb" => {
                let s = self.parse_raw_group();
                return (MBox::text(s.chars().map(symbols::math_bb).collect::<String>()), Kind::Ord, false);
            }
            "mathcal" | "mathscr" => {
                let s = self.parse_raw_group();
                return (MBox::text(s.chars().map(symbols::math_cal).collect::<String>()), Kind::Ord, false);
            }
            "mathfrak" => {
                let s = self.parse_raw_group();
                return (MBox::text(s.chars().map(symbols::math_frak).collect::<String>()), Kind::Ord, false);
            }
            "ce" => {
                let s = self.parse_raw_group();
                return (MBox::text(chemistry(&s)), Kind::Ord, false);
            }
            "begin" => {
                let env = self.parse_raw_group();
                return (self.environment(&env), Kind::Ord, false);
            }
            "left" => {
                let l = self.delimiter();
                let inner = self.parse_seq();
                let mut r = String::new();
                if let Some(Tok::Cmd(c)) = self.peek()
                    && c == "right"
                {
                    self.pos += 1;
                    r = self.delimiter();
                }
                return (MBox::fence(inner, &l, &r), Kind::Ord, false);
            }
            "big" | "Big" | "bigg" | "Bigg" | "bigl" | "Bigl" | "biggl" | "Biggl" | "bigr" | "Bigr" | "biggr" | "Biggr" | "bigm" | "Bigm" => {
                let d = self.delimiter();
                let kind = classify(&d);
                return (MBox::text(d), kind, false);
            }
            "not" => {
                let (b, k, _) = match self.parse_item() {
                    Some((b, k)) => (b, k, false),
                    None => (MBox::empty(), Kind::Ord, false),
                };
                let neg = negate(&flatten(&b));
                return (MBox::text(neg), k, false);
            }
            "substack" => {
                let inner = self.parse_group();
                return (inner, Kind::Ord, false);
            }
            "displaystyle" | "textstyle" | "scriptstyle" | "limits" | "nolimits" | "left." | "right." | "nonumber" | "notag" => {
                return (MBox::empty(), Kind::Tight, false);
            }
            "color" | "textcolor" | "class" | "label" | "tag" | "hspace" | "vspace" | "phantom" | "hphantom" => {
                let _ = self.parse_raw_group();
                return (MBox::empty(), Kind::Tight, false);
            }
            _ => {}
        }
        if let Some(mark) = symbols::accent_mark(name) {
            let inner = self.parse_group();
            return (MBox::accent(inner, mark), Kind::Ord, false);
        }
        if let Some(op) = symbols::big_operator(name) {
            return (MBox::text(op), Kind::Op, true);
        }
        if let Some(op) = symbols::integral(name) {
            return (MBox::text(op), Kind::Op, false);
        }
        if let Some(f) = symbols::function_name(name) {
            return (MBox::text(f), Kind::Op, false);
        }
        if let Some(sym) = symbols::symbol(name) {
            return (MBox::text(sym), classify(sym), false);
        }
        // 모르는 명령은 이름을 남긴다.
        (MBox::text(name.to_string()), Kind::Ord, false)
    }

    /// `\left` / `\right` 뒤의 구분자.
    fn delimiter(&mut self) -> String {
        self.skip_space();
        match self.bump() {
            Some(Tok::Char('.')) => String::new(),
            Some(Tok::Char(c)) => c.to_string(),
            Some(Tok::Open) => "{".to_string(),
            Some(Tok::Close) => "}".to_string(),
            Some(Tok::Cmd(c)) => match c.as_str() {
                "{" | "lbrace" => "{".to_string(),
                "}" | "rbrace" => "}".to_string(),
                "|" | "Vert" => "‖".to_string(),
                "." => String::new(),
                other => symbols::symbol(other).unwrap_or("").to_string(),
            },
            _ => String::new(),
        }
    }

    fn make_frac(&self, num: MBox, den: MBox) -> MBox {
        if self.mode == Mode::Display {
            return MBox::frac(num, den);
        }
        let n = paren_if_compound(&flatten(&num));
        let d = paren_if_compound(&flatten(&den));
        MBox::text(format!("{n}/{d}"))
    }

    /// `\begin{...} ... \end{...}` 환경.
    fn environment(&mut self, env: &str) -> MBox {
        let base = env.trim_end_matches('*');
        let mut aligns: Vec<char> = Vec::new();
        if base == "array" {
            aligns = self.parse_raw_group().chars().filter(|c| matches!(c, 'l' | 'c' | 'r')).collect();
        }
        let rows = self.parse_rows(&aligns);
        // `\end{...}` 소비
        if let Some(Tok::Cmd(c)) = self.peek()
            && c == "end"
        {
            self.pos += 1;
            let _ = self.parse_raw_group();
        }
        let ncol = rows.iter().map(Vec::len).max().unwrap_or(1);
        let (left, right, default_align, gap) = match base {
            "matrix" | "smallmatrix" => ("", "", 'c', 2),
            "pmatrix" => ("(", ")", 'c', 2),
            "bmatrix" => ("[", "]", 'c', 2),
            "Bmatrix" => ("{", "}", 'c', 2),
            "vmatrix" => ("|", "|", 'c', 2),
            "Vmatrix" => ("‖", "‖", 'c', 2),
            "cases" => ("{", "", 'l', 2),
            "aligned" | "align" | "alignat" | "split" => ("", "", 'r', 1),
            "gathered" | "gather" => ("", "", 'c', 2),
            _ => ("", "", 'c', 2),
        };
        if aligns.is_empty() {
            aligns = (0..ncol)
                .map(|i| {
                    if matches!(base, "aligned" | "align" | "alignat" | "split") {
                        if i % 2 == 0 { 'r' } else { 'l' }
                    } else {
                        default_align
                    }
                })
                .collect();
        }
        if self.mode == Mode::Inline {
            // 인라인에서는 행을 `;`로, 열을 공백으로 이어 한 줄로 만든다.
            let text = rows
                .iter()
                .map(|r| r.iter().map(flatten).collect::<Vec<_>>().join(" "))
                .collect::<Vec<_>>()
                .join("; ");
            let l = if left.is_empty() { "" } else { left };
            let r = if right.is_empty() { "" } else { right };
            return MBox::text(format!("{l}{text}{r}"));
        }
        let grid = MBox::grid(rows, &aligns, gap);
        let grid = if base == "cases" { pad_left_cases(grid) } else { grid };
        if left.is_empty() && right.is_empty() { grid } else { MBox::fence(grid, left, right) }
    }
}

/// 케이스 환경은 중괄호와 내용 사이에 한 칸 띄운다.
fn pad_left_cases(b: MBox) -> MBox {
    MBox::text(" ").hcat(b)
}

impl Parser {
    /// 간격 규칙을 적용해 항목들을 잇는다. 첨자 안에서는 붙여 쓴다.
    fn join(&self, items: Vec<(MBox, Kind)>) -> MBox {
    let tight = self.tight > 0;
    let items: Vec<(MBox, Kind)> = items.into_iter().filter(|(b, k)| !(b.is_empty() && *k == Kind::Tight)).collect();
    let mut out = MBox::empty();
    let mut prev: Option<Kind> = None;
    let n = items.len();
    for (i, (b, k)) in items.into_iter().enumerate() {
        let space_before = !tight
            && match k {
                Kind::Rel => prev.is_some(),
                Kind::Bin | Kind::Op => matches!(prev, Some(Kind::Ord) | Some(Kind::Close)),
                _ => false,
            };
        let space_after = i + 1 < n
            && match k {
                Kind::Rel | Kind::Bin => !tight,
                Kind::Op | Kind::Punct => true,
                _ => false,
            };
        if space_before {
            out = out.hcat(MBox::text(" "));
        }
        out = out.hcat(b);
        if space_after {
            out = out.hcat(MBox::text(" "));
        }
        prev = Some(k);
    }
    out
    }
}

fn tok_text(t: &Tok) -> String {
    match t {
        Tok::Char(c) => c.to_string(),
        Tok::Space => " ".to_string(),
        Tok::Cmd(c) => symbols::symbol(c).map(str::to_string).unwrap_or_else(|| match c.as_str() {
            " " => " ".to_string(),
            other => other.to_string(),
        }),
        Tok::Open => "{".to_string(),
        Tok::Close => "}".to_string(),
        Tok::Sup => "^".to_string(),
        Tok::Sub => "_".to_string(),
        Tok::Amp => "&".to_string(),
        Tok::NewRow => " ".to_string(),
    }
}

/// 유니코드 첨자로 바꿀 만큼 짧고 단순한지.
fn unicode_script(b: &MBox, sup: bool) -> Option<String> {
    if b.height() != 1 {
        return None;
    }
    let s = b.lines[0].trim();
    if s.is_empty() {
        return None;
    }
    let simple = s.chars().all(|c| c.is_ascii_digit() || matches!(c, '+' | '-' | '=' | '(' | ')'));
    if !simple && s.chars().count() > 2 {
        return None;
    }
    if sup { symbols::to_superscript(s) } else { symbols::to_subscript(s) }
}

/// 영숫자로만 된 한 줄 첨자.
fn plain_word(b: &MBox) -> Option<String> {
    if b.height() != 1 {
        return None;
    }
    let s = b.lines[0].trim();
    if !s.is_empty() && s.chars().all(char::is_alphanumeric) { Some(s.to_string()) } else { None }
}

/// 여러 줄 상자를 한 줄로 눌러 붙인다.
fn flatten(b: &MBox) -> String {
    if b.height() == 1 {
        return b.lines[0].trim().to_string();
    }
    b.lines.iter().map(|l| l.trim()).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" ")
}

/// 연산자가 섞인 식은 괄호로 감싼다(인라인 분수용).
fn paren_if_compound(s: &str) -> String {
    let compound = s.chars().any(|c| matches!(c, '+' | '-' | '−' | '±' | '/' | ' '));
    if compound { format!("({s})") } else { s.to_string() }
}

/// `\not` 처리.
fn negate(s: &str) -> String {
    match s {
        "=" => "≠".into(),
        "∈" => "∉".into(),
        "⊂" => "⊄".into(),
        "⊆" => "⊈".into(),
        "<" => "≮".into(),
        ">" => "≯".into(),
        "≡" => "≢".into(),
        other => format!("¬{other}"),
    }
}

/// mhchem `\ce{...}` 를 간단히 변환한다.
fn chemistry(s: &str) -> String {
    let s = s.replace("<->", "⇌").replace("<=>", "⇌").replace("->", "→").replace("<-", "←");
    let mut out = String::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_ascii_digit() {
            // 원소 뒤에 붙은 숫자만 아래 첨자로 만든다(계수는 그대로).
            let prev_is_elem = out.chars().last().is_some_and(|p| p.is_ascii_alphabetic() || p == ')');
            if prev_is_elem {
                out.push(symbols::subscript(c).unwrap_or(c));
            } else {
                out.push(c);
            }
        } else if c == '^' && i + 1 < chars.len() {
            let mut j = i + 1;
            let mut sup = String::new();
            while j < chars.len() && (chars[j].is_ascii_digit() || chars[j] == '+' || chars[j] == '-') {
                sup.push(chars[j]);
                j += 1;
            }
            out.push_str(&symbols::to_superscript(&sup).unwrap_or(sup));
            i = j;
            continue;
        } else {
            out.push(c);
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn disp(s: &str) -> Vec<String> {
        build(s, Mode::Display).lines.iter().map(|l| l.trim_end().to_string()).collect()
    }

    #[test]
    fn inline_keeps_one_line() {
        assert_eq!(render_inline("T_{total} = T_{net} + T_{app}"), "T_total = T_net + T_app");
        assert_eq!(render_inline(r"\frac{a}{b}"), "a/b");
        assert_eq!(render_inline(r"1 - 0.999 = 0.1\%"), "1 - 0.999 = 0.1%");
    }

    #[test]
    fn inline_fraction_parenthesizes_sums() {
        assert_eq!(render_inline(r"A = \dfrac{MTBF}{MTBF + MTTR}"), "A = MTBF/(MTBF + MTTR)");
    }

    #[test]
    fn display_fraction_stacks() {
        assert_eq!(disp(r"\frac{1}{n}"), vec![" 1", "───", " n"]);
    }

    #[test]
    fn sum_gets_limits_above_and_below() {
        let out = disp(r"\sum_{i=1}^{n} x_i");
        assert_eq!(out, vec![" n", " ∑  xᵢ", "i=1"]);
    }

    #[test]
    fn greek_and_relations_spaced() {
        assert_eq!(disp(r"\rho = \frac{\lambda}{c\mu}"), vec!["     λ", "ρ = ────", "     cμ"]);
    }

    #[test]
    fn matrix_gets_brackets() {
        let out = disp(r"\begin{bmatrix} a & b \\ c & d \end{bmatrix}");
        assert_eq!(out, vec!["⎡a  b⎤", "⎣c  d⎦"]);
    }

    #[test]
    fn cases_uses_brace() {
        let out = disp(r"\begin{cases} A, & x \ge 1 \\ B, & \text{otherwise} \end{cases}");
        assert_eq!(out[0], "⎧ A,  x ≥ 1");
        assert_eq!(out[1], "⎩ B,  otherwise");
    }

    #[test]
    fn accents_and_roots() {
        assert_eq!(disp(r"\overline{x}"), vec!["─", "x"]);
        assert_eq!(disp(r"\sqrt{2}"), vec!["  ──", "√ 2"]);
    }

    #[test]
    fn chemistry_subscripts() {
        assert_eq!(render_inline(r"\ce{CO2 + H2O -> H2CO3}"), "CO₂ + H₂O → H₂CO₃");
    }

    #[test]
    fn unknown_command_keeps_name() {
        assert_eq!(render_inline(r"\foobar x"), "foobarx");
    }

    #[test]
    fn blackboard_bold() {
        assert_eq!(render_inline(r"W \in \mathbb{R}^{m \times n}"), "W ∈ ℝ^(m×n)");
    }
}

#[cfg(test)]
mod robustness {
    use super::*;

    #[test]
    fn malformed_latex_does_not_panic() {
        let cases = [
            "",
            "{",
            "}",
            "\\frac",
            "\\frac{1}",
            "\\begin{bmatrix}",
            "\\begin{cases} a \\\\",
            "\\left(",
            "\\right)",
            "x^",
            "_",
            "^^__",
            "\\sqrt[",
            "\\text{",
            "a & b \\\\ c",
            "\\unknowncmd{x}",
            "\\ce{",
            "\\\\",
            "%주석만",
            "\\begin{array}{cc} 1 & 2 \\end{array}",
        ];
        for src in cases {
            let _ = render_inline(src);
            let _ = render_block(src, &crate::theme::Theme::dark(), 40);
            let _ = render_block(src, &crate::theme::Theme::notty(), 1);
        }
    }
}
