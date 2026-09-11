//! 계층형 그래프 배치: flowchart · stateDiagram · erDiagram · classDiagram 공용.
//!
//! Sugiyama 방식을 단순화해서 쓴다. (1) 되돌아가는 간선을 뒤집어 DAG로 만들고
//! (2) 최장 경로로 층을 매기고 (3) 무게중심으로 층 안 순서를 정한 뒤
//! (4) 층 사이 띠(band)에 간선을 겹치지 않게 배선한다. 터미널은 세로로 길기
//! 때문에 방향 지정과 무관하게 항상 위에서 아래로 배치한다.

use crate::doc::{Line, Style};
use crate::render::canvas::{Canvas, truncate, width_of};
use crate::theme::Theme;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shape {
    Rect,
    Round,
    Diamond,
    Cylinder,
    Subroutine,
    Circle,
}

/// 간선 양 끝 모양.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Arrow {
    None,
    /// 보통 화살표
    Open,
    /// 속 빈 삼각형(상속)
    Hollow,
    /// 채운 마름모(합성)
    Diamond,
    /// 빈 마름모(집합)
    HollowDiamond,
    /// ×(실패·소멸)
    Cross,
}

#[derive(Clone, Debug)]
pub struct GNode {
    pub id: String,
    pub lines: Vec<String>,
    pub shape: Shape,
    pub group: Option<usize>,
    /// 본문을 왼쪽 맞춤으로 쓸지(클래스·ER 박스).
    pub left_align: bool,
    /// 첫 줄을 제목으로 강조할지.
    pub header: bool,
}

/// 상자 안 가로 구분선을 나타내는 한 줄.
pub const DIVIDER: &str = "\u{1}";

impl GNode {
    pub fn new(id: impl Into<String>, label: &str, shape: Shape) -> Self {
        GNode {
            id: id.into(),
            lines: label.lines().map(str::to_string).collect(),
            shape,
            group: None,
            left_align: false,
            header: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct GEdge {
    pub from: usize,
    pub to: usize,
    pub label: String,
    pub dashed: bool,
    pub thick: bool,
    /// 도착 쪽 머리
    pub head: Arrow,
    /// 출발 쪽 머리
    pub tail: Arrow,
    /// 도착 쪽에 붙는 짧은 글자(ER 카디널리티)
    pub head_tag: String,
    pub tail_tag: String,
}

impl GEdge {
    pub fn new(from: usize, to: usize) -> Self {
        GEdge {
            from,
            to,
            label: String::new(),
            dashed: false,
            thick: false,
            head: Arrow::Open,
            tail: Arrow::None,
            head_tag: String::new(),
            tail_tag: String::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Group {
    pub title: String,
    pub members: Vec<usize>,
}

#[derive(Default)]
pub struct Graph {
    pub nodes: Vec<GNode>,
    pub edges: Vec<GEdge>,
    pub groups: Vec<Group>,
}

// ---------------------------------------------------------------- 배치 결과

#[derive(Clone, Debug)]
struct Placed {
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    rank: usize,
    /// 긴 간선을 잇기 위한 가상 노드
    dummy: bool,
    node: usize,
}

const HGAP: usize = 3;
const GROUP_PAD: usize = 2;

impl Graph {
    /// 라벨 폭을 줄여가며 터미널 폭 안에 들어오는 배치를 찾는다.
    pub fn render(&self, theme: &Theme, width: usize) -> Vec<Line> {
        for cap in [30usize, 24, 20, 16, 12, 9] {
            let boxes = self.sizes(cap);
            let plan = self.place(&boxes);
            if plan.total_w <= width || cap == 9 {
                return self.draw(&plan, theme);
            }
        }
        Vec::new()
    }

    /// 노드별 (폭, 높이, 줄 내용).
    fn sizes(&self, cap: usize) -> Vec<(usize, usize, Vec<String>)> {
        self.nodes
            .iter()
            .map(|n| {
                let mut lines: Vec<String> = Vec::new();
                for raw in &n.lines {
                    if raw == DIVIDER {
                        lines.push(DIVIDER.to_string());
                    } else {
                        lines.extend(wrap_label(raw, cap.max(if n.left_align { 22 } else { 0 })));
                    }
                }
                if lines.is_empty() {
                    lines.push(String::new());
                }
                let w = lines.iter().filter(|l| *l != DIVIDER).map(|l| width_of(l)).max().unwrap_or(0) + 4;
                let h = lines.len() + 2;
                (w.max(5), h, lines)
            })
            .collect()
    }

    /// 층을 매긴다.
    ///
    /// 서브그래프는 한 덩어리로 다룬다. 먼저 덩어리끼리 층을 매겨 겹치지 않는
    /// 층 구간을 나눠 준 뒤, 덩어리 안에서 다시 층을 매긴다. 이렇게 해야
    /// 서브그래프 테두리가 서로 포개지지 않는다.
    fn rank_nodes(&self) -> Vec<usize> {
        let n = self.nodes.len();
        // 노드 → 덩어리. 서브그래프에 속하지 않으면 자기 자신이 덩어리다.
        let ng = self.groups.len();
        let cluster: Vec<usize> = (0..n).map(|i| self.nodes[i].group.unwrap_or(ng + i)).collect();
        let ncl = ng + n;

        // 덩어리 안에서의 층
        let intra: Vec<(usize, usize)> = self
            .edges
            .iter()
            .filter(|e| e.from != e.to && cluster[e.from] == cluster[e.to])
            .map(|e| (e.from, e.to))
            .collect();
        let local = longest_path(n, &intra);
        let mut height = vec![1usize; ncl];
        for i in 0..n {
            height[cluster[i]] = height[cluster[i]].max(local[i] + 1);
        }

        // 덩어리끼리의 층
        let inter: Vec<(usize, usize)> = self
            .edges
            .iter()
            .filter(|e| cluster[e.from] != cluster[e.to])
            .map(|e| (cluster[e.from], cluster[e.to]))
            .collect();
        let crank = longest_path(ncl, &inter);

        // 덩어리 시작 층: 앞선 덩어리들의 높이를 누적한다.
        let mut order: Vec<usize> = (0..ncl).collect();
        order.sort_by_key(|c| crank[*c]);
        let mut base = vec![0usize; ncl];
        for &c in &order {
            for &(a, b) in &inter {
                if b == c && crank[a] < crank[c] {
                    base[c] = base[c].max(base[a] + height[a]);
                }
            }
        }
        (0..n).map(|i| base[cluster[i]] + local[i]).collect()
    }

    /// 층 안에서의 순서를 무게중심으로 정하고, 같은 서브그래프끼리 붙인다.
    fn order_ranks(&self, rank: &[usize], nranks: usize) -> Vec<Vec<usize>> {
        let mut ranks: Vec<Vec<usize>> = vec![Vec::new(); nranks];
        for (i, r) in rank.iter().enumerate() {
            ranks[*r].push(i);
        }
        let mut pos: Vec<usize> = vec![0; self.nodes.len()];
        for r in &ranks {
            for (i, v) in r.iter().enumerate() {
                pos[*v] = i;
            }
        }
        for pass in 0..4 {
            let down = pass % 2 == 0;
            let order: Vec<usize> = if down { (1..nranks).collect() } else { (0..nranks.saturating_sub(1)).rev().collect() };
            for r in order {
                let mut keyed: Vec<(f32, usize, usize)> = ranks[r]
                    .iter()
                    .enumerate()
                    .map(|(i, &v)| {
                        let mut sum = 0f32;
                        let mut cnt = 0f32;
                        for e in &self.edges {
                            let other = if e.to == v && down {
                                Some(e.from)
                            } else if e.from == v && !down {
                                Some(e.to)
                            } else {
                                None
                            };
                            if let Some(o) = other
                                && rank[o] != rank[v]
                            {
                                sum += pos[o] as f32;
                                cnt += 1.0;
                            }
                        }
                        let bc = if cnt > 0.0 { sum / cnt } else { i as f32 };
                        (bc, i, v)
                    })
                    .collect();
                keyed.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal).then(a.1.cmp(&b.1)));
                ranks[r] = keyed.iter().map(|k| k.2).collect();
                for (i, v) in ranks[r].iter().enumerate() {
                    pos[*v] = i;
                }
            }
        }
        // 서브그래프 구성원을 한데 모은다.
        for r in ranks.iter_mut() {
            let mut mean: std::collections::HashMap<usize, (f32, f32)> = std::collections::HashMap::new();
            for (i, v) in r.iter().enumerate() {
                if let Some(g) = self.nodes.get(*v).and_then(|n| n.group) {
                    let e = mean.entry(g).or_insert((0.0, 0.0));
                    e.0 += i as f32;
                    e.1 += 1.0;
                }
            }
            let mut keyed: Vec<(f32, usize, usize)> = r
                .iter()
                .enumerate()
                .map(|(i, &v)| {
                    let key = match self.nodes.get(v).and_then(|n| n.group) {
                        Some(g) => mean.get(&g).map(|(s, c)| s / c).unwrap_or(i as f32),
                        None => i as f32,
                    };
                    (key, i, v)
                })
                .collect();
            keyed.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal).then(a.1.cmp(&b.1)));
            *r = keyed.iter().map(|k| k.2).collect();
        }
        ranks
    }

    // 층·블록 번호로 여러 배열을 함께 훑기 때문에 색인 순회가 더 읽기 쉽다.
    #[allow(clippy::needless_range_loop)]
    fn place(&self, boxes: &[(usize, usize, Vec<String>)]) -> Plan {
        let n = self.nodes.len();
        let rank = self.rank_nodes();
        let nranks = rank.iter().copied().max().map(|m| m + 1).unwrap_or(1);
        let ranks = self.order_ranks(&rank, nranks);

        // 앞으로 가는 간선만 층 사이에 배선한다. 긴 간선은 가상 노드로 쪼갠다.
        // 되돌아가는 간선(뒤 층 → 앞 층)은 오른쪽 여백을 타고 올라간다.
        let mut ranks: Vec<Vec<usize>> = ranks;
        let mut dummy_rank: Vec<usize> = Vec::new();
        let mut segments: Vec<Seg> = Vec::new();
        let mut feedback: Vec<usize> = Vec::new();
        let mut selfloop: Vec<usize> = Vec::new();
        for (ei, e) in self.edges.iter().enumerate() {
            if e.from == e.to {
                selfloop.push(ei);
                continue;
            }
            let (ra, rb) = (rank[e.from], rank[e.to]);
            if ra >= rb {
                feedback.push(ei);
                continue;
            }
            let (a, b) = (e.from, e.to);
            let mut chain: Vec<usize> = Vec::new();
            if rb - ra > 1 {
                for (r, rank_ids) in ranks.iter_mut().enumerate().take(rb).skip(ra + 1) {
                    let id = n + dummy_rank.len();
                    dummy_rank.push(r);
                    rank_ids.push(id);
                    chain.push(id);
                }
            }
            let mut prev = a;
            for &d in &chain {
                segments.push(Seg { edge: ei, top: prev, bottom: d });
                prev = d;
            }
            segments.push(Seg { edge: ei, top: prev, bottom: b });
        }

        // 가상 노드는 앞 노드 아래에 오도록 위치를 조정한다.
        let total = n + dummy_rank.len();
        let mut px = vec![0usize; total];
        let mut pw = vec![1usize; total];
        let mut ph = vec![1usize; total];
        for i in 0..n {
            pw[i] = boxes[i].0;
            ph[i] = boxes[i].1;
        }

        // 층별 세로 위치와 높이
        let mut rank_h = vec![1usize; nranks];
        for (r, ids) in ranks.iter().enumerate() {
            rank_h[r] = ids.iter().map(|&i| ph[i]).max().unwrap_or(1);
        }

        // 가로 위치는 두 단계로 정한다.
        //  (1) 서브그래프(덩어리) 안에서 구성원끼리 자리를 잡고
        //  (2) 덩어리를 하나의 블록으로 보고 층별로 왼쪽부터 늘어놓는다.
        // 이렇게 하면 서브그래프 테두리가 서로 포개질 수 없다.
        let ng = self.groups.len();
        // 노드 → 블록. 서브그래프 밖의 노드와 가상 노드는 저마다 하나의 블록이다.
        let item_of: Vec<usize> = (0..total)
            .map(|i| match self.nodes.get(i).and_then(|nd| nd.group) {
                Some(g) => g,
                None => ng + i,
            })
            .collect();
        let nitems = ng + total;
        let mut members: Vec<Vec<usize>> = vec![Vec::new(); nitems];
        for i in 0..total {
            members[item_of[i]].push(i);
        }

        // (1) 블록 안에서의 상대 위치
        for it in 0..nitems {
            if members[it].len() <= 1 {
                if let Some(&i) = members[it].first() {
                    px[i] = 0;
                }
                continue;
            }
            let sub: Vec<Vec<usize>> = ranks.iter().map(|ids| ids.iter().copied().filter(|&i| item_of[i] == it).collect()).collect();
            for ids in &sub {
                let mut x = 0;
                for (k, &i) in ids.iter().enumerate() {
                    if k > 0 {
                        x += HGAP;
                    }
                    px[i] = x;
                    x += pw[i];
                }
            }
            let inner: Vec<Seg> = segments.iter().copied().filter(|sg| item_of[sg.top] == it && item_of[sg.bottom] == it).collect();
            for _ in 0..3 {
                for r in 1..nranks {
                    self.align_rank(&sub, r, &inner, &mut px, &pw, true);
                }
                for r in (0..nranks.saturating_sub(1)).rev() {
                    self.align_rank(&sub, r, &inner, &mut px, &pw, false);
                }
            }
            let base = members[it].iter().map(|&i| px[i]).min().unwrap_or(0);
            for &i in &members[it] {
                px[i] -= base;
            }
        }
        let item_w: Vec<usize> = (0..nitems).map(|it| members[it].iter().map(|&i| px[i] + pw[i]).max().unwrap_or(0)).collect();
        let item_span: Vec<Option<(usize, usize)>> = (0..nitems)
            .map(|it| {
                let rs: Vec<usize> = members[it].iter().map(|&i| node_rank(i, n, &rank, &dummy_rank)).collect();
                rs.iter().copied().min().map(|lo| (lo, rs.iter().copied().max().unwrap_or(lo)))
            })
            .collect();

        // (2) 블록을 층별로 왼쪽부터 늘어놓는다. 여러 층에 걸친 블록은 모든 층에서 같은 자리를 쓴다.
        let mut item_x: Vec<Option<usize>> = vec![None; nitems];
        let mut cursor = vec![0usize; nranks];
        for r in 0..nranks {
            let mut seen: Vec<usize> = Vec::new();
            for &i in &ranks[r] {
                let it = item_of[i];
                if !seen.contains(&it) {
                    seen.push(it);
                }
            }
            for it in seen {
                if item_x[it].is_some() {
                    continue;
                }
                let Some((lo, hi)) = item_span[it] else { continue };
                let grouped = it < ng;
                let pad = if grouped { GROUP_PAD } else { 0 };
                let off = (lo..=hi).map(|r2| cursor[r2]).max().unwrap_or(0) + pad;
                item_x[it] = Some(off);
                for c in cursor.iter_mut().take(hi + 1).skip(lo) {
                    *c = off + item_w[it] + pad + HGAP;
                }
            }
        }
        for i in 0..total {
            px[i] += item_x[item_of[i]].unwrap_or(0);
        }

        // 되돌아가는 간선이 쓸 세로 통로 위치를 먼저 잡는다.
        let body_w = (0..n).map(|i| px[i] + pw[i]).max().unwrap_or(0) + 1;
        let gutters: Vec<(usize, usize)> = feedback.iter().enumerate().map(|(i, &ei)| (ei, body_w + 1 + i * 2)).collect();

        // 띠(band)에 간선 배선 — 가로 구간이 겹치지 않게 줄을 나눈다.
        let mut band_rows = vec![1usize; nranks];
        let mut seg_row: Vec<usize> = vec![0; segments.len()];
        let mut fb_row: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
        for r in 0..nranks {
            let mut intervals: Vec<(usize, usize, usize)> = Vec::new(); // (시작, 끝, 구간번호)
            for (si, seg) in segments.iter().enumerate() {
                let (ei, a, b) = (seg.edge, seg.top, seg.bottom);
                let ra = node_rank(a, n, &rank, &dummy_rank);
                if ra != r {
                    continue;
                }
                let (ax, bx) = (center(px[a], pw[a]), center(px[b], pw[b]));
                let lab = width_of(&self.edges[ei].label);
                let lo = ax.min(bx);
                let hi = ax.max(bx) + if lab > 0 { lab + 2 } else { 0 };
                intervals.push((lo, hi, si));
            }
            // 되돌아가는 간선의 가로 구간도 같은 띠를 쓴다.
            for &(ei, gx) in &gutters {
                let src = self.edges[ei].from;
                if rank[src] != r {
                    continue;
                }
                let lo = center(px[src], pw[src]);
                let lab = width_of(&self.edges[ei].label);
                intervals.push((lo, gx.max(lo + lab + 2), usize::MAX - ei));
            }
            intervals.sort_by_key(|t| t.0);
            let mut ends: Vec<usize> = Vec::new();
            for (lo, hi, si) in intervals {
                let slot = ends.iter().position(|&e| e + 1 < lo).unwrap_or(ends.len());
                if slot == ends.len() {
                    ends.push(hi);
                } else {
                    ends[slot] = hi;
                }
                if si > segments.len() {
                    fb_row.insert(usize::MAX - si, slot);
                } else {
                    seg_row[si] = slot;
                }
            }
            band_rows[r] = ends.len().max(1);
        }

        // 서브그래프 테두리를 위한 여유
        let group_ranks = self.group_rank_spans(&ranks, n);
        let mut rank_y: Vec<usize> = vec![0; nranks];
        let mut band_start = vec![0usize; nranks];
        let mut y = 0usize;
        for r in 0..nranks {
            if group_ranks.iter().any(|(_, lo, _)| *lo == r) {
                y += 2;
            }
            rank_y[r] = y;
            y += rank_h[r];
            if group_ranks.iter().any(|(_, _, hi)| *hi == r) {
                y += 1;
            }
            band_start[r] = y;
            if r + 1 < nranks {
                y += band_rows[r] + 2;
            }
        }
        let total_h = y;

        let mut placed: Vec<Placed> = Vec::with_capacity(total);
        for i in 0..total {
            let r = node_rank(i, n, &rank, &dummy_rank);
            let h = if i < n { ph[i] } else { rank_h[r] };
            placed.push(Placed { x: px[i], y: rank_y[r], w: pw[i], h, rank: r, dummy: i >= n, node: i });
        }

        self.separate_groups(&mut placed, n);
        self.compact_x(&mut placed);

        // 왼쪽 여백 정리
        let minx = placed.iter().map(|p| p.x).min().unwrap_or(0);
        let pad = if self.groups.is_empty() { 0 } else { 2 };
        for p in placed.iter_mut() {
            p.x = p.x - minx + pad;
        }
        // 서브그래프를 떼어놓느라 폭이 달라졌으므로 통로 위치를 다시 잡는다.
        let body_w = placed.iter().filter(|p| !p.dummy).map(|p| p.x + p.w).max().unwrap_or(0) + 2;
        let gutters: Vec<(usize, usize)> = gutters.iter().enumerate().map(|(i, (ei, _))| (*ei, body_w + i * 2)).collect();
        let total_w = gutters.last().map(|(_, x)| x + 2).unwrap_or(body_w) + GROUP_PAD;

        Plan {
            placed,
            segments,
            seg_row,
            band_rows,
            band_start,
            gutters,
            fb_row,
            selfloop,
            total_w,
            total_h: total_h + 1,
            lines: boxes.iter().map(|b| b.2.clone()).collect(),
        }
    }

    /// 이웃 층 평균 위치로 노드를 당겨 간선을 곧게 만든다.
    fn align_rank(&self, ranks: &[Vec<usize>], r: usize, segments: &[Seg], px: &mut [usize], pw: &[usize], down: bool) {
        let ids = &ranks[r];
        let mut desired: Vec<(usize, usize)> = Vec::with_capacity(ids.len());
        for &i in ids {
            let mut sum = 0usize;
            let mut cnt = 0usize;
            for seg in segments {
                let (a, b) = (seg.top, seg.bottom);
                let other = if down && b == i {
                    Some(a)
                } else if !down && a == i {
                    Some(b)
                } else {
                    None
                };
                if let Some(o) = other {
                    sum += center(px[o], pw[o]);
                    cnt += 1;
                }
            }
            let want = match sum.checked_div(cnt) {
                Some(avg) => avg.saturating_sub(pw[i] / 2),
                None => px[i],
            };
            desired.push((i, want));
        }
        // 왼쪽부터 최소 간격을 지키며 배치
        let mut cursor = 0usize;
        let mut first = true;
        for (i, want) in desired {
            let x = if first { want } else { want.max(cursor) };
            px[i] = x;
            cursor = x + pw[i] + HGAP;
            first = false;
        }
    }

    /// 아무것도 없는 세로 띠가 길게 남으면 줄여서 그림을 다시 모은다.
    fn compact_x(&self, placed: &mut [Placed]) {
        const MAX_GAP: usize = 5;
        let maxx = placed.iter().map(|p| p.x + p.w).max().unwrap_or(0);
        let mut occ = vec![false; maxx + 2];
        for p in placed.iter() {
            for slot in &mut occ[p.x..p.x + p.w] {
                *slot = true;
            }
        }
        // 서브그래프 테두리가 지나는 자리도 비어 있다고 보지 않는다.
        for g in &self.groups {
            let ms: Vec<&Placed> = placed.iter().filter(|p| !p.dummy && g.members.contains(&p.node)).collect();
            if ms.is_empty() {
                continue;
            }
            let a = ms.iter().map(|p| p.x).min().unwrap_or(0).saturating_sub(2);
            let b = (ms.iter().map(|p| p.x + p.w).max().unwrap_or(0) + 1).min(maxx);
            for slot in &mut occ[a..=b] {
                *slot = true;
            }
        }
        let mut map = vec![0usize; maxx + 2];
        let mut cur = 0usize;
        let mut run = 0usize;
        for x in 0..=maxx + 1 {
            map[x] = cur;
            if x <= maxx && occ[x] {
                cur += 1;
                run = 0;
            } else {
                run += 1;
                if run <= MAX_GAP {
                    cur += 1;
                }
            }
        }
        for p in placed.iter_mut() {
            p.x = map[p.x];
        }
    }

    /// 세로로 겹치는 서브그래프끼리 가로 영역이 포개지지 않게 민다.
    fn separate_groups(&self, placed: &mut [Placed], n: usize) {
        if self.groups.len() < 2 {
            return;
        }
        for _ in 0..self.groups.len() {
            let boxes: Vec<Option<(usize, usize, usize, usize)>> = self
                .groups
                .iter()
                .map(|g| {
                    let ms: Vec<&Placed> = placed.iter().filter(|p| !p.dummy && g.members.contains(&p.node)).collect();
                    if ms.is_empty() {
                        return None;
                    }
                    Some((
                        ms.iter().map(|p| p.x).min().unwrap_or(0).saturating_sub(2),
                        ms.iter().map(|p| p.x + p.w).max().unwrap_or(0) + 1,
                        ms.iter().map(|p| p.y).min().unwrap_or(0).saturating_sub(2),
                        ms.iter().map(|p| p.y + p.h).max().unwrap_or(0),
                    ))
                })
                .collect();
            let mut shift: Option<(usize, usize)> = None; // (기준 x, 밀 거리)
            'outer: for a in 0..boxes.len() {
                for b in 0..boxes.len() {
                    let (Some(ba), Some(bb)) = (boxes[a], boxes[b]) else { continue };
                    if a == b || ba.0 > bb.0 {
                        continue;
                    }
                    let overlap_y = ba.2 < bb.3 && bb.2 < ba.3;
                    let overlap_x = ba.0 < bb.1 && bb.0 < ba.1;
                    if overlap_y && overlap_x {
                        shift = Some((bb.0, ba.1 + 2 - bb.0));
                        break 'outer;
                    }
                }
            }
            let Some((from_x, delta)) = shift else { return };
            for p in placed.iter_mut() {
                if p.x >= from_x && (p.dummy || p.node < n) {
                    p.x += delta;
                }
            }
        }
    }

    /// 서브그래프가 걸쳐 있는 층 범위.
    fn group_rank_spans(&self, ranks: &[Vec<usize>], n: usize) -> Vec<(usize, usize, usize)> {
        let mut out = Vec::new();
        for (gi, g) in self.groups.iter().enumerate() {
            let mut lo = usize::MAX;
            let mut hi = 0usize;
            for (r, ids) in ranks.iter().enumerate() {
                if ids.iter().any(|&i| i < n && g.members.contains(&i)) {
                    lo = lo.min(r);
                    hi = hi.max(r);
                }
            }
            if lo != usize::MAX {
                out.push((gi, lo, hi));
            }
        }
        out
    }

    fn draw(&self, plan: &Plan, theme: &Theme) -> Vec<Line> {
        let mut c = Canvas::new(plan.total_w + 2, plan.total_h + 2);
        let border = theme.diagram_border;
        let edge_st = theme.diagram_edge;
        let label_st = theme.diagram_label;

        // 1) 서브그래프 테두리
        let mut group_titles: Vec<(usize, usize, String)> = Vec::new();
        for (gi, g) in self.groups.iter().enumerate() {
            let members: Vec<&Placed> = plan.placed.iter().filter(|p| !p.dummy && g.members.contains(&p.node)).collect();
            if members.is_empty() {
                continue;
            }
            let x0 = members.iter().map(|p| p.x).min().unwrap_or(0).saturating_sub(2);
            let x1 = members.iter().map(|p| p.x + p.w).max().unwrap_or(0) + 1;
            let y0 = members.iter().map(|p| p.y).min().unwrap_or(0).saturating_sub(2);
            let y1 = members.iter().map(|p| p.y + p.h).max().unwrap_or(0);
            let st = theme.diagram_note;
            let title = truncate(&g.title, 28);
            let w = (x1 + 1 - x0).max(width_of(&title) + 6);
            let h = y1 + 1 - y0;
            c.rect(x0, y0, w, h, st, true);
            if !title.is_empty() {
                group_titles.push((x0 + 2, y0, format!(" {title} ")));
            }
            let _ = gi;
        }

        // 2) 간선
        let mut labels: Vec<(usize, usize, String)> = Vec::new();
        for (si, seg) in plan.segments.iter().enumerate() {
            let e = &self.edges[seg.edge];
            let upper = &plan.placed[seg.top];
            let lower = &plan.placed[seg.bottom];
            let band_y = plan.band_start[upper.rank];
            let row = band_y + plan.seg_row[si].min(plan.band_rows[upper.rank].saturating_sub(1));
            let ux = center(upper.x, upper.w);
            let lx = center(lower.x, lower.w);
            // 한두 칸 어긋난 정도면 곧은 세로선으로 맞춘다(라벨이 선을 끊지 않게).
            let lx = if ux.abs_diff(lx) <= 1 { ux } else { lx };
            let (line_v, line_h) = if e.dashed {
                ('╎', '╌')
            } else if e.thick {
                ('┃', '━')
            } else {
                ('│', '─')
            };

            let from_y = if upper.dummy { upper.y } else { upper.y + upper.h };
            let to_y = if lower.dummy { lower.y + lower.h - 1 } else { lower.y.saturating_sub(1) };
            if ux == lx {
                c.vline(ux, from_y, to_y, line_v, edge_st);
            } else {
                if row > from_y {
                    c.vline(ux, from_y, row - 1, line_v, edge_st);
                }
                c.hline(ux.min(lx) + 1, ux.max(lx) - 1, row, line_h, edge_st);
                if to_y > row {
                    c.vline(lx, row + 1, to_y, line_v, edge_st);
                }
                // 꺾이는 자리는 모서리 문자로.
                let (a, b) = if lx > ux { ('└', '┐') } else { ('┘', '┌') };
                c.draw(ux, row, a, edge_st);
                c.draw(lx, row, b, edge_st);
            }

            // 화살표: 위 끝은 항상 위쪽을, 아래 끝은 아래쪽을 가리킨다.
            let is_first = plan.segments[..si].iter().all(|s| s.edge != seg.edge);
            let is_last = plan.segments[si + 1..].iter().all(|s| s.edge != seg.edge);
            if is_last && !lower.dummy {
                if let Some(ch) = arrow_char(e.head, true) {
                    c.set(lx, to_y, ch, edge_st);
                }
                if !e.head_tag.is_empty() {
                    c.text(lx + 2, to_y, &e.head_tag, theme.diagram_note);
                }
            }
            if is_first && !upper.dummy {
                if let Some(ch) = arrow_char(e.tail, false) {
                    c.set(ux, from_y, ch, edge_st);
                }
                if !e.tail_tag.is_empty() {
                    c.text(ux + 2, from_y, &e.tail_tag, theme.diagram_note);
                }
            }

            // 라벨은 가로 구간 가운데(또는 세로선 옆)에 놓되, 선을 다 그린 뒤 얹는다.
            if !e.label.is_empty() && is_first {
                let lab = truncate(&e.label, 24);
                let lw = width_of(&lab);
                // 가로 구간이 라벨보다 짧으면 가운데 놓을 수 없다. 선 오른쪽에 붙인다.
                let lx0 = if ux.abs_diff(lx) < lw {
                    ux.max(lx) + 2
                } else {
                    (ux.min(lx) + ux.max(lx)).div_ceil(2).saturating_sub(lw / 2)
                };
                labels.push((lx0, row, lab));
            }
        }

        // 2b) 되돌아가는 간선: 아래에서 나와 오른쪽 통로를 타고 올라가 옆구리로 들어간다.
        for &(ei, gx) in &plan.gutters {
            let e = &self.edges[ei];
            let (src, tgt) = (&plan.placed[e.from], &plan.placed[e.to]);
            let (line_v, line_h) = if e.dashed { ('╎', '╌') } else { ('│', '─') };
            let sy = plan.band_start[src.rank] + plan.fb_row.get(&ei).copied().unwrap_or(0);
            let ty = tgt.y + tgt.h / 2;
            let sx = center(src.x, src.w);
            c.hline(sx + 1, gx - 1, sy, line_h, edge_st);
            c.vline(gx, ty + 1, sy - 1, line_v, edge_st);
            c.hline(tgt.x + tgt.w + 1, gx - 1, ty, line_h, edge_st);
            c.draw(sx, sy, '└', edge_st);
            c.draw(gx, sy, '┘', edge_st);
            c.draw(gx, ty, '┐', edge_st);
            if arrow_char(e.head, true).is_some() {
                c.set(tgt.x + tgt.w, ty, '◀', edge_st);
            }
            if !e.label.is_empty() {
                labels.push((sx + 2, sy, truncate(&e.label, 20)));
            }
        }

        // 2d) 서브그래프 제목과 간선 라벨은 선 위에 올려 쓴다.
        for (x, y, t) in &group_titles {
            c.text(*x, *y, t, theme.diagram_note);
        }
        for (x, y, lab) in &labels {
            for i in 0..width_of(lab) + 2 {
                c.set((x + i).saturating_sub(1), *y, ' ', edge_st);
            }
            c.text(*x, *y, lab, theme.diagram_note);
        }

        // 2c) 자기 자신으로 가는 간선
        for &ei in &plan.selfloop {
            let e = &self.edges[ei];
            let p = &plan.placed[e.from];
            let y = p.y + p.h / 2;
            let mut text = String::from("↺");
            if !e.label.is_empty() {
                text.push(' ');
                text.push_str(&truncate(&e.label, 18));
            }
            c.text(p.x + p.w + 1, y, &text, theme.diagram_note);
        }

        // 3) 노드 상자
        for p in plan.placed.iter() {
            if p.dummy {
                continue;
            }
            let node = &self.nodes[p.node];
            c.clear_rect(p.x, p.y, p.w, p.h);
            draw_shape(&mut c, p.x, p.y, p.w, p.h, node.shape, border);
            for (i, text) in plan.lines[p.node].iter().enumerate() {
                let y = p.y + 1 + i;
                if text == DIVIDER {
                    c.hline(p.x, p.x + p.w - 1, y, '─', border);
                    c.set(p.x, y, '├', border);
                    c.set(p.x + p.w - 1, y, '┤', border);
                    continue;
                }
                let head = node.header && i == 0;
                let st = if head { theme.diagram_title } else { label_st };
                let inner = p.w.saturating_sub(2);
                // 구분선 앞쪽(이름·스테레오타입)은 가운데, 본문은 왼쪽에 맞춘다.
                let before_divider = plan.lines[p.node][..i].iter().all(|l| l != DIVIDER);
                let centered = !node.left_align || head || before_divider;
                let off = if centered { (inner.saturating_sub(width_of(text))) / 2 } else { 1 };
                c.text(p.x + 1 + off, y, text, st);
            }
        }
        c.into_lines()
    }
}

/// 한 층만큼 내려가는 간선 조각.
#[derive(Clone, Copy)]
struct Seg {
    edge: usize,
    top: usize,
    bottom: usize,
}

struct Plan {
    placed: Vec<Placed>,
    segments: Vec<Seg>,
    seg_row: Vec<usize>,
    band_rows: Vec<usize>,
    /// 층별 간선 배선이 시작되는 줄
    band_start: Vec<usize>,
    /// (간선 번호, 세로 통로 x)
    gutters: Vec<(usize, usize)>,
    /// 되돌아가는 간선이 쓰는 띠 줄 번호
    fb_row: std::collections::HashMap<usize, usize>,
    selfloop: Vec<usize>,
    total_w: usize,
    total_h: usize,
    lines: Vec<Vec<String>>,
}

/// 되돌아가는 간선을 걷어낸 뒤 최장 경로로 층을 매긴다.
fn longest_path(n: usize, edges: &[(usize, usize)]) -> Vec<usize> {
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for &(a, b) in edges {
        if a != b && a < n && b < n {
            adj[a].push(b);
        }
    }
    // DFS로 역방향 간선을 찾는다.
    let mut state = vec![0u8; n];
    let mut back: Vec<(usize, usize)> = Vec::new();
    for s in 0..n {
        if state[s] != 0 {
            continue;
        }
        state[s] = 1;
        let mut stack = vec![(s, 0usize)];
        while let Some((v, i)) = stack.pop() {
            if i < adj[v].len() {
                stack.push((v, i + 1));
                let w = adj[v][i];
                match state[w] {
                    0 => {
                        state[w] = 1;
                        stack.push((w, 0));
                    }
                    1 => back.push((v, w)),
                    _ => {}
                }
            } else {
                state[v] = 2;
            }
        }
    }
    let mut indeg = vec![0usize; n];
    let mut fwd: Vec<Vec<usize>> = vec![Vec::new(); n];
    for &(a, b) in edges {
        if a == b || a >= n || b >= n || back.contains(&(a, b)) {
            continue;
        }
        fwd[a].push(b);
        indeg[b] += 1;
    }
    let mut rank = vec![0usize; n];
    let mut queue: Vec<usize> = (0..n).filter(|i| indeg[*i] == 0).collect();
    while let Some(v) = queue.pop() {
        for &w in &fwd[v] {
            rank[w] = rank[w].max(rank[v] + 1);
            indeg[w] -= 1;
            if indeg[w] == 0 {
                queue.push(w);
            }
        }
    }
    rank
}

fn node_rank(i: usize, n: usize, rank: &[usize], dummy_rank: &[usize]) -> usize {
    if i < n { rank[i] } else { dummy_rank[i - n] }
}

fn center(x: usize, w: usize) -> usize {
    x + w / 2
}

fn arrow_char(a: Arrow, down: bool) -> Option<char> {
    Some(match (a, down) {
        (Arrow::None, _) => return None,
        (Arrow::Open, true) => '▼',
        (Arrow::Open, false) => '▲',
        (Arrow::Hollow, true) => '▽',
        (Arrow::Hollow, false) => '△',
        (Arrow::Diamond, _) => '◆',
        (Arrow::HollowDiamond, _) => '◇',
        (Arrow::Cross, _) => '✕',
    })
}

fn draw_shape(c: &mut Canvas, x: usize, y: usize, w: usize, h: usize, shape: Shape, st: Style) {
    match shape {
        Shape::Rect => c.rect(x, y, w, h, st, false),
        Shape::Round | Shape::Circle | Shape::Diamond => c.rect(x, y, w, h, st, true),
        Shape::Cylinder => {
            c.rect(x, y, w, h, st, true);
            c.hline(x + 1, x + w - 2, y, '═', st);
            c.hline(x + 1, x + w - 2, y + h - 1, '═', st);
        }
        Shape::Subroutine => {
            c.rect(x, y, w, h, st, false);
            for yy in y + 1..y + h - 1 {
                c.set(x + 1, yy, '│', st);
                c.set(x + w - 2, yy, '│', st);
            }
        }
    }
    if shape == Shape::Diamond {
        c.set(x, y, '◇', st);
    }
}

/// 라벨 한 줄을 폭에 맞춰 나눈다(공백 우선, 없으면 글자 단위).
pub fn wrap_label(s: &str, cap: usize) -> Vec<String> {
    let s = s.trim();
    if s.is_empty() {
        return vec![String::new()];
    }
    if width_of(s) <= cap {
        return vec![s.to_string()];
    }
    let mut out = Vec::new();
    let mut cur = String::new();
    for word in s.split_whitespace() {
        let ww = width_of(word);
        if cur.is_empty() {
            cur = word.to_string();
        } else if width_of(&cur) + 1 + ww <= cap {
            cur.push(' ');
            cur.push_str(word);
        } else {
            out.push(std::mem::take(&mut cur));
            cur = word.to_string();
        }
        while width_of(&cur) > cap {
            let mut head = String::new();
            let mut rest = String::new();
            let mut wsum = 0;
            for ch in cur.chars() {
                let cw = width_of(&ch.to_string());
                if wsum + cw <= cap && rest.is_empty() {
                    head.push(ch);
                    wsum += cw;
                } else {
                    rest.push(ch);
                }
            }
            out.push(head);
            cur = rest;
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Line;

    fn plain(g: &Graph, w: usize) -> Vec<String> {
        g.render(&Theme::notty(), w).iter().map(Line::plain).collect()
    }

    #[test]
    fn two_nodes_stack_vertically() {
        let mut g = Graph::default();
        g.nodes.push(GNode::new("A", "A", Shape::Rect));
        g.nodes.push(GNode::new("B", "B", Shape::Rect));
        g.edges.push(GEdge::new(0, 1));
        let out = plain(&g, 40);
        assert_eq!(out[0].trim(), "┌───┐");
        assert!(out.iter().any(|l| l.contains('▼')), "arrow head missing: {out:?}");
        assert!(out.iter().filter(|l| l.contains("─┐")).count() >= 2);
    }

    #[test]
    fn wrap_label_splits_on_spaces() {
        assert_eq!(wrap_label("one two three", 7), vec!["one two", "three"]);
        assert_eq!(wrap_label("abcdefghij", 4), vec!["abcd", "efgh", "ij"]);
    }

    #[test]
    fn siblings_sit_side_by_side() {
        let mut g = Graph::default();
        g.nodes.push(GNode::new("A", "A", Shape::Rect));
        g.nodes.push(GNode::new("B", "B", Shape::Rect));
        g.nodes.push(GNode::new("C", "C", Shape::Rect));
        g.edges.push(GEdge::new(0, 1));
        g.edges.push(GEdge::new(0, 2));
        let out = plain(&g, 60);
        let row = out.iter().find(|l| l.matches('B').count() + l.matches('C').count() == 2).unwrap();
        assert!(row.contains('B') && row.contains('C'));
    }
}
