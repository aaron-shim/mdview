//! 마크다운 파일 브라우저 TUI: Local 탭(현재 디렉터리)과 Stashed 탭(즐겨찾기).
//!
//! 목록은 평면/트리 두 가지로 볼 수 있고(`v`), 오른쪽에 미리보기 창을 붙일 수
//! 있다(`p`). 이동은 vi 키(j/k, Ctrl-d/u/f/b, gg, G)를 따르고, 트리는 h/l 로
//! 접고 편다(H/L 은 전체). 미리보기 창 폭은 구분선을 마우스로 끌어 바꾼다.

use crate::output::tree::{self, Row};
use crate::output::tui_convert;
use crate::output::watch::{Snapshot, Watcher};
use crate::source::{self, Source, Stamp};
use crate::stash::Stash;
use crate::theme::Theme;
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::{DefaultTerminal, Frame};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Local,
    Stashed,
}

/// 목록에 잠시 붙여 두는 변경 표시.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Change {
    Added,
    Modified,
}

/// 키 입력을 받는 창.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Focus {
    List,
    Preview,
}

/// 목록 표시 방식.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    /// 경로를 한 줄씩 나열
    Flat,
    /// 디렉터리 아래에 파일을 계층으로
    Tree,
}

enum Mode {
    Normal,
    Filter,
    /// 스태시 항목 메모 편집 (대상 인덱스, 입력 중인 텍스트)
    Note(usize, String),
    /// 이동할 폴더 경로 입력
    GoTo(String),
}

/// 미리보기로 그려 둔 내용. 같은 (경로, 폭)이면 다시 렌더링하지 않는다.
struct PreviewCache {
    key: (String, usize),
    /// 렌더링할 때의 파일 표식. 달라지면 같은 문서라도 다시 그린다.
    stamp: Option<Stamp>,
    lines: Vec<Line<'static>>,
}

pub struct Picker {
    root: PathBuf,
    files: Vec<PathBuf>,
    stash: Rc<RefCell<Stash>>,
    theme: Theme,
    tab: Tab,
    view: View,
    rows: Vec<Row>,
    state: ListState,
    filter: String,
    mode: Mode,
    status: Option<(String, Instant)>,
    /// 접어 둔 디렉터리(트리 보기)
    collapsed: HashSet<PathBuf>,
    preview: bool,
    cache: Option<PreviewCache>,
    /// 지금 키를 받는 창
    focus: Focus,
    /// 미리보기 창의 스크롤 위치(줄 단위)
    preview_scroll: usize,
    /// 마지막으로 그릴 때 미리보기가 실제로 보였는지
    preview_visible: bool,
    /// 마지막으로 그린 미리보기 높이
    preview_height: usize,
    /// 지금 미리보기에 담긴 전체 줄 수
    preview_len: usize,
    /// `gg` 입력 대기
    pending_g: bool,
    /// 마지막으로 그린 목록 높이(페이지 단위 이동에 쓴다)
    page: usize,
    /// 목록 창이 차지하는 폭(본문 폭 대비 백분율). 구분선을 끌어 바꾼다.
    split_pct: u16,
    /// 구분선을 끌고 있는 중인지
    dragging: bool,
    /// 마지막으로 그린 본문·목록·미리보기 영역(마우스 판정에 쓴다)
    body_area: Rect,
    list_area: Rect,
    preview_area: Option<Rect>,
    /// 목록 자동 갱신용 백그라운드 감시기(테스트에서는 없다)
    watcher: Option<Watcher>,
    /// 마지막으로 반영한 파일 표식. 다음 스냅숏과 비교하는 기준
    stamps: Vec<(PathBuf, Stamp)>,
    /// `stamps`가 지금 루트 기준으로 유효한지(루트를 바꾸면 첫 결과는 비교하지 않는다)
    baseline: bool,
    /// 최근에 생기거나 바뀐 파일(루트 기준 상대 경로)
    changed: HashMap<PathBuf, (Change, Instant)>,
    /// 상위로 올라온 뒤 커서를 둘 폴더 이름. 새 루트를 다 훑을 때까지 형제 폴더를 접어 두는 기준이기도 하다.
    came_from: Option<PathBuf>,
    /// 올라온 뒤 그 폴더에 커서를 이미 놓았는지(부분 결과가 올 때마다 다시 옮기지 않는다)
    landed: bool,
    /// 새 루트를 훑는 중인지
    scanning: bool,
}

pub enum PickerExit {
    Quit,
    /// (소스, 표시 제목)
    Open(Source, String),
}

const STATUS_TTL: Duration = Duration::from_secs(2);
/// 미리보기를 붙이기 위한 최소 가로 폭
const MIN_WIDTH_FOR_PREVIEW: u16 = 76;
/// 미리보기로 읽을 최대 파일 크기
const MAX_PREVIEW_BYTES: u64 = 1 << 20;
/// 목록 창의 기본 폭(백분율)
const DEFAULT_SPLIT_PCT: u16 = 42;
/// 구분선을 끌 때 목록·미리보기가 각각 지켜야 하는 최소 폭
const MIN_LIST_COLS: u16 = 20;
const MIN_PREVIEW_COLS: u16 = 30;
/// 새로 생기거나 바뀐 파일에 표시를 붙여 두는 시간
const CHANGE_TTL: Duration = Duration::from_secs(30);

impl Picker {
    pub fn new(root: PathBuf, files: Vec<PathBuf>, stash: Rc<RefCell<Stash>>, theme: Theme) -> Self {
        let mut p = Picker {
            root,
            files,
            stash,
            theme,
            tab: Tab::Local,
            view: View::Tree,
            rows: Vec::new(),
            state: ListState::default(),
            filter: String::new(),
            mode: Mode::Normal,
            status: None,
            collapsed: HashSet::new(),
            preview: true,
            cache: None,
            focus: Focus::List,
            preview_scroll: 0,
            preview_visible: false,
            preview_height: 10,
            preview_len: 0,
            pending_g: false,
            page: 10,
            split_pct: DEFAULT_SPLIT_PCT,
            dragging: false,
            body_area: Rect::default(),
            list_area: Rect::default(),
            preview_area: None,
            watcher: None,
            stamps: Vec::new(),
            baseline: false,
            changed: HashMap::new(),
            came_from: None,
            landed: false,
            scanning: false,
        };
        p.rebuild();
        p.select_first_item();
        p
    }

    // ------------------------------------------------------------ 목록 구성

    fn item_count(&self) -> usize {
        match self.tab {
            Tab::Local => self.files.len(),
            Tab::Stashed => self.stash.borrow().entries.len(),
        }
    }

    fn item_text(&self, i: usize) -> String {
        match self.tab {
            Tab::Local => crate::hangul::compose(&self.files[i].to_string_lossy()),
            Tab::Stashed => {
                let st = self.stash.borrow();
                let e = &st.entries[i];
                crate::hangul::compose(&format!("{} {}", e.source, e.note.as_deref().unwrap_or("")))
            }
        }
    }

    /// 필터·보기 방식·접힘 상태를 반영해 행을 다시 만든다.
    fn rebuild(&mut self) {
        let had_parent = matches!(self.rows.first(), Some(Row::Parent));
        let q = self.filter.to_lowercase();
        let keep: Vec<usize> = (0..self.item_count()).filter(|&i| q.is_empty() || self.item_text(i).to_lowercase().contains(&q)).collect();
        self.rows = match self.tab {
            Tab::Local if self.view == View::Tree => {
                let entries: Vec<(usize, PathBuf)> = keep.iter().map(|&i| (i, self.files[i].clone())).collect();
                // 필터 중에는 결과가 숨지 않도록 모두 펼친다.
                let collapsed = if q.is_empty() { self.collapsed.clone() } else { HashSet::new() };
                tree::rows(&entries, &collapsed)
            }
            _ => tree::flat_rows(&keep.iter().map(|&i| (i, PathBuf::new())).collect::<Vec<_>>()),
        };
        // Local 탭 맨 위에 상위 폴더로 가는 `../` 행을 둔다. 필터 중에는 숨긴다.
        let has_parent = self.tab == Tab::Local && q.is_empty() && self.root.parent().is_some();
        if has_parent {
            self.rows.insert(0, Row::Parent);
        }
        if self.rows.is_empty() {
            self.state.select(None);
        } else {
            // `../` 행이 생기거나 사라져도 같은 항목을 가리키도록 한 칸 보정한다.
            let mut sel = self.state.selected().unwrap_or(0);
            match (had_parent, has_parent) {
                (false, true) if self.state.selected().is_some() => sel += 1,
                (true, false) => sel = sel.saturating_sub(1),
                _ => {}
            }
            self.state.select(Some(sel.min(self.rows.len() - 1)));
        }
    }

    /// 첫 번째 "파일" 행으로 커서를 옮긴다(트리 첫 줄은 디렉터리일 수 있다).
    fn select_first_item(&mut self) {
        if let Some(i) = self.rows.iter().position(|r| r.item_index().is_some()) {
            self.state.select(Some(i));
        }
    }

    fn switch_tab(&mut self, tab: Tab) {
        if self.tab != tab {
            self.tab = tab;
            self.filter.clear();
            self.state.select(Some(0));
            self.rebuild();
            self.select_first_item();
        }
    }

    fn toggle_view(&mut self) {
        if self.tab != Tab::Local {
            self.set_status("트리 보기는 Local 탭에서만 쓸 수 있습니다");
            return;
        }
        // 보고 있던 파일을 그대로 따라가도록 기억해 둔다.
        let current = self.selected_item();
        self.view = if self.view == View::Tree { View::Flat } else { View::Tree };
        self.rebuild();
        match current.and_then(|i| self.rows.iter().position(|r| r.item_index() == Some(i))) {
            Some(pos) => self.state.select(Some(pos)),
            None => self.select_first_item(),
        }
        self.set_status(if self.view == View::Tree { "트리 보기" } else { "평면 보기" });
    }

    // ------------------------------------------------------------ 이동

    fn move_sel(&mut self, delta: isize) {
        if self.rows.is_empty() {
            return;
        }
        let cur = self.state.selected().unwrap_or(0) as isize;
        let n = self.rows.len() as isize;
        self.state.select(Some((cur + delta).clamp(0, n - 1) as usize));
    }

    /// 미리보기 창을 위아래로 움직인다. 페이저와 같은 방식으로 끝에서 멈춘다.
    fn scroll_preview(&mut self, delta: isize) {
        let max = self.preview_len.saturating_sub(self.preview_height);
        let next = (self.preview_scroll as isize + delta).clamp(0, max as isize);
        self.preview_scroll = next as usize;
    }

    fn scroll_preview_to(&mut self, pos: Pos) {
        self.preview_scroll = match pos {
            Pos::First => 0,
            Pos::Last => self.preview_len.saturating_sub(self.preview_height),
        };
    }

    /// 미리보기 창으로 들어간다. 꺼져 있으면 켜고 들어간다.
    fn enter_preview(&mut self) {
        if !self.preview {
            self.preview = true;
            self.preview_visible = true; // 다음 draw 에서 실제 폭을 보고 정해진다
        }
        if self.preview_visible {
            self.focus = Focus::Preview;
        } else {
            self.set_status("창이 좁아 미리보기를 열 수 없습니다");
        }
    }

    fn goto(&mut self, pos: Pos) {
        if self.rows.is_empty() {
            return;
        }
        let last = self.rows.len() - 1;
        self.state.select(Some(match pos {
            Pos::First => 0,
            Pos::Last => last,
        }));
    }

    // ------------------------------------------------------------ 선택 항목

    /// 선택된 행이 파일이면 원본 인덱스.
    fn selected_item(&self) -> Option<usize> {
        self.state.selected().and_then(|s| self.rows.get(s)).and_then(Row::item_index)
    }

    /// 선택된 행이 디렉터리이면 그 경로.
    fn selected_dir(&self) -> Option<PathBuf> {
        match self.state.selected().and_then(|s| self.rows.get(s)) {
            Some(Row::Dir { path, .. }) => Some(path.clone()),
            _ => None,
        }
    }

    /// 선택된 파일의 실제 경로(미리보기·열기에 쓴다).
    fn selected_path(&self) -> Option<Source> {
        let i = self.selected_item()?;
        Some(match self.tab {
            Tab::Local => Source::File(self.root.join(&self.files[i])),
            Tab::Stashed => Source::from_stash_key(&self.stash.borrow().entries[i].source),
        })
    }

    fn set_status(&mut self, msg: impl Into<String>) {
        self.status = Some((msg.into(), Instant::now()));
    }

    fn toggle_dir(&mut self) {
        let Some(path) = self.selected_dir() else { return };
        if !self.collapsed.remove(&path) {
            self.collapsed.insert(path);
        }
        let sel = self.state.selected();
        self.rebuild();
        if let Some(s) = sel {
            self.state.select(Some(s.min(self.rows.len().saturating_sub(1))));
        }
    }

    /// `h`: 열린 디렉터리는 접고, 접힌 디렉터리나 파일 위에서는 부모 디렉터리로 올라간다.
    fn collapse_or_parent(&mut self) {
        if matches!(self.state.selected().and_then(|s| self.rows.get(s)), Some(Row::Dir { open: true, .. })) {
            self.toggle_dir();
            return;
        }
        let Some(sel) = self.state.selected() else { return };
        let depth = self.rows[sel].depth();
        if depth == 0 {
            return;
        }
        if let Some(pos) = self.rows[..sel].iter().rposition(|r| matches!(r, Row::Dir { depth: d, .. } if *d < depth)) {
            self.state.select(Some(pos));
        }
    }

    /// `l`: 디렉터리면 펴고(이미 열려 있으면 첫 하위 항목으로), 파일이면 미리보기 창으로 들어간다.
    fn expand_or_enter_preview(&mut self) {
        if self.selected_dir().is_some() {
            self.expand();
        } else {
            self.enter_preview();
        }
    }

    /// 트리의 모든 디렉터리 경로(조상 포함).
    fn all_dirs(&self) -> HashSet<PathBuf> {
        let mut dirs = HashSet::new();
        for f in &self.files {
            let mut p = f.parent();
            while let Some(d) = p {
                if d.as_os_str().is_empty() {
                    break;
                }
                dirs.insert(d.to_path_buf());
                p = d.parent();
            }
        }
        dirs
    }

    /// `H`: 모든 디렉터리를 접는다. 커서는 보고 있던 항목이 속한 최상위 디렉터리에 둔다.
    fn collapse_all(&mut self) {
        if self.tab != Tab::Local || self.view != View::Tree {
            return;
        }
        let top = self.state.selected().and_then(|s| self.rows.get(s)).and_then(|r| match r {
            Row::Dir { path, .. } => Some(path.clone()),
            Row::Item { idx, .. } => Some(self.files[*idx].clone()),
            Row::Parent => None,
        });
        self.collapsed = self.all_dirs();
        self.rebuild();
        let top_dir = top.and_then(|p| p.components().next().map(|c| PathBuf::from(c.as_os_str())));
        match top_dir.and_then(|d| self.rows.iter().position(|r| matches!(r, Row::Dir { path, .. } if *path == d))) {
            Some(pos) => self.state.select(Some(pos)),
            None => self.goto(Pos::First),
        }
        self.set_status("모든 디렉터리 접음");
    }

    /// `L`: 모든 디렉터리를 편다. 커서는 같은 항목을 계속 가리킨다.
    fn expand_all(&mut self) {
        if self.tab != Tab::Local || self.view != View::Tree {
            return;
        }
        let current = self.state.selected().and_then(|s| self.rows.get(s)).cloned();
        self.collapsed.clear();
        self.rebuild();
        let pos = current.and_then(|cur| {
            self.rows.iter().position(|r| match (&cur, r) {
                (Row::Dir { path: a, .. }, Row::Dir { path: b, .. }) => a == b,
                (Row::Item { idx: a, .. }, Row::Item { idx: b, .. }) => a == b,
                _ => false,
            })
        });
        match pos {
            Some(pos) => self.state.select(Some(pos)),
            None => self.select_first_item(),
        }
        self.set_status("모든 디렉터리 폄");
    }

    fn expand(&mut self) {
        if let Some(path) = self.selected_dir() {
            if self.collapsed.remove(&path) {
                let sel = self.state.selected();
                self.rebuild();
                if let Some(s) = sel {
                    self.state.select(Some(s));
                }
            } else {
                self.move_sel(1);
            }
        }
    }

    fn stash_selected(&mut self) {
        let Some(i) = self.selected_item() else { return };
        let path = self.root.join(&self.files[i]);
        let Some(key) = Source::File(path).stash_key() else { return };
        let result = {
            let mut st = self.stash.borrow_mut();
            let added = st.add(&key, None);
            st.save().map(|_| added)
        };
        match result {
            Ok(true) => self.set_status("Stashed ✓  (Tab to view)"),
            Ok(false) => self.set_status("Already stashed"),
            Err(e) => self.set_status(format!("Stash failed: {e:#}")),
        }
    }

    fn remove_selected(&mut self) {
        let Some(i) = self.selected_item() else { return };
        let result = {
            let mut st = self.stash.borrow_mut();
            st.remove(i);
            st.save()
        };
        match result {
            Ok(()) => self.set_status("Removed from stash"),
            Err(e) => self.set_status(format!("Save failed: {e:#}")),
        }
        self.rebuild();
    }

    // ------------------------------------------------------------ 마우스

    fn on_divider(&self, x: u16) -> bool {
        self.preview_area.is_some_and(|p| x + 1 >= p.x && x <= p.x + 1)
    }

    /// 구분선을 `x` 열로 옮긴다. 양쪽 창의 최소 폭은 지킨다.
    fn set_split_at(&mut self, x: u16) {
        let body = self.body_area;
        if body.width < MIN_LIST_COLS + MIN_PREVIEW_COLS {
            return;
        }
        let rel = x.saturating_sub(body.x).clamp(MIN_LIST_COLS, body.width - MIN_PREVIEW_COLS);
        self.split_pct = (u32::from(rel) * 100 / u32::from(body.width)) as u16;
    }

    /// 본문 폭에 맞춘 목록 창 폭.
    fn list_width(&self, body: Rect) -> u16 {
        let w = (u32::from(body.width) * u32::from(self.split_pct) / 100) as u16;
        w.clamp(MIN_LIST_COLS, body.width.saturating_sub(MIN_PREVIEW_COLS).max(MIN_LIST_COLS))
    }

    fn on_mouse(&mut self, m: MouseEvent) {
        let (x, y) = (m.column, m.row);
        let in_list = rect_contains(self.list_area, x, y);
        let in_preview = self.preview_area.is_some_and(|a| rect_contains(a, x, y));
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if self.on_divider(x) {
                    self.dragging = true;
                } else if in_list {
                    self.focus = Focus::List;
                    let row = usize::from(y - self.list_area.y) + self.state.offset();
                    if row < self.rows.len() {
                        self.state.select(Some(row));
                    }
                } else if in_preview {
                    self.focus = Focus::Preview;
                }
            }
            MouseEventKind::Drag(MouseButton::Left) if self.dragging => self.set_split_at(x),
            MouseEventKind::Up(_) => self.dragging = false,
            MouseEventKind::ScrollDown => {
                if in_preview {
                    self.scroll_preview(3);
                } else if in_list {
                    self.move_sel(1);
                }
            }
            MouseEventKind::ScrollUp => {
                if in_preview {
                    self.scroll_preview(-3);
                } else if in_list {
                    self.move_sel(-1);
                }
            }
            _ => {}
        }
    }

    // ------------------------------------------------------------ 상위 폴더 · 자동 갱신

    /// 백그라운드에서 루트를 다시 훑어 목록을 자동으로 갱신한다. `scan`은 이미 보여 준 목록.
    pub fn start_watching(&mut self, scan: Vec<(PathBuf, Stamp)>) {
        self.watcher = Some(Watcher::spawn(self.root.clone(), scan.clone()));
        self.stamps = scan;
        self.baseline = true;
    }

    fn on_parent_row(&self) -> bool {
        matches!(self.state.selected().and_then(|s| self.rows.get(s)), Some(Row::Parent))
    }

    /// 상위 폴더로 올라간다. 올라오기 전 폴더에 커서를 두고, 나머지 형제 폴더는 접어 둔다.
    fn go_parent(&mut self) {
        if self.tab != Tab::Local {
            return;
        }
        let Some(parent) = self.root.parent().map(Path::to_path_buf) else {
            self.set_status("최상위 폴더입니다");
            return;
        };
        let from = self.root.file_name().map(PathBuf::from);
        // 접어 두었던 폴더는 새 루트 기준 경로로 옮겨 그대로 유지하고,
        // 방금 보던 하위 트리는 새 루트 기준으로 옮겨 훑기가 끝나기 전에 먼저 보여 준다.
        let seed = match &from {
            Some(name) => {
                self.collapsed = self.collapsed.iter().map(|p| name.join(p)).collect();
                self.files.iter().map(|f| name.join(f)).collect()
            }
            None => Vec::new(),
        };
        self.came_from = from;
        self.set_status(format!("상위 폴더: {}", short(&parent)));
        self.set_root(parent, seed);
    }

    /// 입력한 경로(`Mode::GoTo`)로 루트를 옮긴다. 잘못된 경로면 알리고 입력창은 그대로 둔다.
    fn go_to(&mut self) {
        let Mode::GoTo(text) = &self.mode else { return };
        if self.tab != Tab::Local {
            self.mode = Mode::Normal;
            return;
        }
        match resolve_dir(&self.root, text) {
            Ok(dir) => {
                self.mode = Mode::Normal;
                self.collapsed.clear();
                self.came_from = None;
                self.set_status(format!("이동: {}", short(&dir)));
                self.set_root(dir, Vec::new());
            }
            Err(e) => self.set_status(e),
        }
    }

    /// 루트를 바꾸고 목록을 다시 훑는다. 상태 메시지와 `came_from`은 호출한 쪽에서 미리 정한다.
    /// `seed`는 훑은 결과가 오기 전까지 먼저 보여 줄 목록(새 루트 기준 상대 경로).
    fn set_root(&mut self, root: PathBuf, seed: Vec<PathBuf>) {
        self.root = root.clone();
        self.filter.clear();
        self.changed.clear();
        self.baseline = false;
        self.landed = false;
        self.cache = None;
        self.focus = Focus::List;
        match &self.watcher {
            Some(w) => {
                // 넓은 폴더는 훑는 데 시간이 걸린다. 화면을 멈추지 않고 결과를 기다린다.
                w.set_root(root);
                self.files = seed;
                self.stamps.clear();
                self.scanning = true;
                self.rebuild();
                self.land();
            }
            None => {
                let files = source::scan_markdown_files(&root);
                self.apply_snapshot(Snapshot { root, files, done: true });
            }
        }
    }

    /// 올라오기 전 폴더(`came_from`)가 목록에 있으면 커서를 그리로 옮긴다. 한 번만.
    fn land(&mut self) {
        if self.landed {
            return;
        }
        let Some(from) = &self.came_from else {
            self.select_first_item();
            return;
        };
        match self.rows.iter().position(|r| matches!(r, Row::Dir { path, .. } if path == from)) {
            Some(pos) => {
                self.state.select(Some(pos));
                self.landed = true;
            }
            None => self.select_first_item(),
        }
    }

    /// 새로 훑은 결과를 목록에 반영한다. 커서는 같은 항목을 계속 가리킨다.
    /// 부분 결과(`done == false`)면 목록만 채우고 훑는 중 표시는 그대로 둔다.
    fn apply_snapshot(&mut self, snap: Snapshot) {
        // 루트를 바꾸기 전에 출발한 늦은 결과는 버린다.
        if snap.root != self.root {
            return;
        }
        let anchor = self.anchor();
        if self.baseline {
            let d = crate::output::watch::diff(&self.stamps, &snap.files);
            let now = Instant::now();
            for p in &d.added {
                self.changed.insert(p.clone(), (Change::Added, now));
            }
            for p in &d.modified {
                self.changed.insert(p.clone(), (Change::Modified, now));
            }
            for p in &d.removed {
                self.changed.remove(p);
            }
            if !d.is_empty() {
                self.set_status(d.summary());
            }
        }
        self.files = snap.files.iter().map(|(p, _)| p.clone()).collect();
        self.stamps = snap.files;
        // 완료본이 와야 다음 결과와 비교할 기준이 된다. 부분 결과끼리 비교하면 죄다 '새 파일'이 된다.
        self.baseline = snap.done;
        self.scanning = !snap.done;

        if let Some(from) = self.came_from.clone() {
            // 올라온 폴더만 펼치고 형제 폴더는 접어 둔다. 훑기가 끝날 때까지 새로 나타나는 형제도 마찬가지.
            for f in &self.files {
                if f.components().count() > 1
                    && let Some(top) = f.components().next().map(|c| PathBuf::from(c.as_os_str()))
                    && top != from
                {
                    self.collapsed.insert(top);
                }
            }
            self.rebuild();
            if self.landed {
                self.restore(anchor);
            } else {
                self.land();
            }
            if snap.done {
                self.came_from = None;
            }
            return;
        }
        self.rebuild();
        self.restore(anchor);
    }

    /// 목록이 바뀌어도 커서를 되찾기 위한 표식.
    fn anchor(&self) -> Option<Anchor> {
        match self.state.selected().and_then(|s| self.rows.get(s))? {
            Row::Parent => Some(Anchor::Parent),
            Row::Dir { path, .. } => Some(Anchor::Dir(path.clone())),
            Row::Item { idx, .. } if self.tab == Tab::Local => self.files.get(*idx).cloned().map(Anchor::File),
            Row::Item { .. } => None,
        }
    }

    fn restore(&mut self, anchor: Option<Anchor>) {
        let Some(a) = anchor else { return };
        let pos = self.rows.iter().position(|r| match (&a, r) {
            (Anchor::Parent, Row::Parent) => true,
            (Anchor::Dir(p), Row::Dir { path, .. }) => p == path,
            (Anchor::File(p), Row::Item { idx, .. }) => self.files.get(*idx) == Some(p),
            _ => false,
        });
        // 보던 파일이 지워졌으면 rebuild 가 맞춰 둔 위치에 남는다.
        if let Some(pos) = pos {
            self.state.select(Some(pos));
        }
    }

    /// 오래된 변경 표시를 지운다.
    fn prune_changes(&mut self) {
        self.changed.retain(|_, (_, t)| t.elapsed() < CHANGE_TTL);
    }

    fn open_selected(&self) -> Option<PickerExit> {
        let i = self.selected_item()?;
        Some(match self.tab {
            Tab::Local => {
                let rel = &self.files[i];
                PickerExit::Open(Source::File(self.root.join(rel)), crate::hangul::compose(&rel.display().to_string()))
            }
            Tab::Stashed => {
                let e = self.stash.borrow().entries[i].clone();
                let title = if e.is_url() { e.source.clone() } else { crate::stash::shorten_home(&e.source) };
                PickerExit::Open(Source::from_stash_key(&e.source), title)
            }
        })
    }

    // ------------------------------------------------------------ 입력 처리

    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<PickerExit> {
        // 페이저에서 돌아왔을 때 스태시가 바뀌었을 수 있다.
        self.rebuild();
        loop {
            // 백그라운드에서 훑은 최신 목록이 있으면 먼저 반영한다.
            if let Some(snap) = self.watcher.as_ref().and_then(Watcher::latest) {
                self.apply_snapshot(snap);
            }
            terminal.draw(|f| self.draw(f))?;
            if !event::poll(Duration::from_millis(250))? {
                continue;
            }
            let k = match event::read()? {
                Event::Key(k) if k.kind != KeyEventKind::Release => k,
                Event::Mouse(m) => {
                    if matches!(self.mode, Mode::Normal) {
                        self.on_mouse(m);
                    }
                    continue;
                }
                _ => continue,
            };
            let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
            match &mut self.mode {
                Mode::Filter => {
                    match k.code {
                        KeyCode::Esc => {
                            self.mode = Mode::Normal;
                            self.filter.clear();
                            self.rebuild();
                        }
                        KeyCode::Enter => self.mode = Mode::Normal,
                        KeyCode::Backspace => {
                            self.filter.pop();
                            self.rebuild();
                        }
                        KeyCode::Char(c) if !ctrl => {
                            self.filter.push(c);
                            self.rebuild();
                            self.select_first_item();
                        }
                        KeyCode::Down => self.move_sel(1),
                        KeyCode::Up => self.move_sel(-1),
                        _ => {}
                    }
                    continue;
                }
                Mode::Note(idx, text) => {
                    match k.code {
                        KeyCode::Esc => self.mode = Mode::Normal,
                        KeyCode::Enter => {
                            let (idx, text) = (*idx, std::mem::take(text));
                            let result = {
                                let mut st = self.stash.borrow_mut();
                                st.set_note(idx, text);
                                st.save()
                            };
                            self.mode = Mode::Normal;
                            match result {
                                Ok(()) => self.set_status("Note saved"),
                                Err(e) => self.set_status(format!("Save failed: {e:#}")),
                            }
                        }
                        KeyCode::Backspace => {
                            text.pop();
                        }
                        KeyCode::Char(c) if !ctrl => text.push(c),
                        _ => {}
                    }
                    continue;
                }
                Mode::GoTo(text) => {
                    match k.code {
                        KeyCode::Esc => self.mode = Mode::Normal,
                        KeyCode::Enter => self.go_to(),
                        KeyCode::Tab => {
                            if let Some(done) = complete_dir(&self.root, text) {
                                *text = done;
                            }
                        }
                        KeyCode::Backspace => {
                            text.pop();
                        }
                        KeyCode::Char('u') if ctrl => text.clear(),
                        KeyCode::Char(c) if !ctrl => text.push(c),
                        _ => {}
                    }
                    continue;
                }
                Mode::Normal => {}
            }
            // `gg`는 g를 두 번 눌러야 한다. 다른 키가 오면 대기를 푼다.
            let was_pending_g = std::mem::take(&mut self.pending_g);
            let half = (self.page / 2).max(1) as isize;
            let full = self.page.max(1) as isize;
            // 미리보기 창에 들어가 있으면 같은 이동 키가 그쪽을 움직인다.
            let in_preview = self.focus == Focus::Preview;
            let (half_p, full_p) = ((self.preview_height / 2).max(1) as isize, self.preview_height.max(1) as isize);
            match k.code {
                KeyCode::Char('g') if !ctrl => {
                    if !was_pending_g {
                        self.pending_g = true;
                    } else if in_preview {
                        self.scroll_preview_to(Pos::First);
                    } else {
                        self.goto(Pos::First);
                    }
                }
                KeyCode::Char('q') => return Ok(PickerExit::Quit),
                KeyCode::Char('c') if ctrl => return Ok(PickerExit::Quit),
                KeyCode::Esc => {
                    if in_preview {
                        self.focus = Focus::List;
                    } else {
                        return Ok(PickerExit::Quit);
                    }
                }
                // 목록 쪽 동작을 부르는 키는 먼저 포커스를 목록으로 되돌린다.
                KeyCode::Tab | KeyCode::BackTab => {
                    self.focus = Focus::List;
                    let next = if self.tab == Tab::Local { Tab::Stashed } else { Tab::Local };
                    self.switch_tab(next);
                }
                KeyCode::Char('1') => {
                    self.focus = Focus::List;
                    self.switch_tab(Tab::Local);
                }
                KeyCode::Char('2') => {
                    self.focus = Focus::List;
                    self.switch_tab(Tab::Stashed);
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    if in_preview {
                        self.scroll_preview(1)
                    } else {
                        self.move_sel(1)
                    }
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    if in_preview {
                        self.scroll_preview(-1)
                    } else {
                        self.move_sel(-1)
                    }
                }
                KeyCode::Char('d') if ctrl => {
                    if in_preview {
                        self.scroll_preview(half_p)
                    } else {
                        self.move_sel(half)
                    }
                }
                KeyCode::Char('u') if ctrl => {
                    if in_preview {
                        self.scroll_preview(-half_p)
                    } else {
                        self.move_sel(-half)
                    }
                }
                KeyCode::Char('f') if ctrl => {
                    if in_preview {
                        self.scroll_preview(full_p)
                    } else {
                        self.move_sel(full)
                    }
                }
                KeyCode::Char('b') if ctrl => {
                    if in_preview {
                        self.scroll_preview(-full_p)
                    } else {
                        self.move_sel(-full)
                    }
                }
                KeyCode::Char(' ') | KeyCode::PageDown => {
                    if in_preview {
                        self.scroll_preview(full_p)
                    } else {
                        self.move_sel(full)
                    }
                }
                KeyCode::PageUp => {
                    if in_preview {
                        self.scroll_preview(-full_p)
                    } else {
                        self.move_sel(-full)
                    }
                }
                KeyCode::Char('G') | KeyCode::End => {
                    if in_preview {
                        self.scroll_preview_to(Pos::Last)
                    } else {
                        self.goto(Pos::Last)
                    }
                }
                KeyCode::Home => {
                    if in_preview {
                        self.scroll_preview_to(Pos::First)
                    } else {
                        self.goto(Pos::First)
                    }
                }
                KeyCode::Char('v') => {
                    self.focus = Focus::List;
                    self.toggle_view();
                }
                KeyCode::Char('p') => {
                    self.preview = !self.preview;
                    if !self.preview {
                        self.focus = Focus::List;
                    }
                    self.set_status(if self.preview { "미리보기 켬" } else { "미리보기 끔" });
                }
                // h/l: 트리에서 접기/펴기. 파일 위의 l 은 미리보기로, 미리보기 안의 h 는 목록으로.
                KeyCode::Char('h') | KeyCode::Left => {
                    if in_preview {
                        self.focus = Focus::List;
                    } else {
                        self.collapse_or_parent();
                    }
                }
                KeyCode::Char('l') | KeyCode::Right => {
                    if !in_preview {
                        self.expand_or_enter_preview();
                    }
                }
                KeyCode::Char('H') => {
                    self.focus = Focus::List;
                    self.collapse_all();
                }
                KeyCode::Char('L') => {
                    self.focus = Focus::List;
                    self.expand_all();
                }
                KeyCode::Char('/') => {
                    self.focus = Focus::List;
                    self.mode = Mode::Filter;
                }
                KeyCode::Char('s') if self.tab == Tab::Local => self.stash_selected(),
                KeyCode::Char('c') if self.tab == Tab::Local => {
                    self.focus = Focus::List;
                    self.mode = Mode::GoTo(String::new());
                }
                KeyCode::Char('x') if self.tab == Tab::Stashed => {
                    self.focus = Focus::List;
                    self.remove_selected();
                }
                KeyCode::Char('m') if self.tab == Tab::Stashed => {
                    if let Some(i) = self.selected_item() {
                        let cur = self.stash.borrow().entries[i].note.clone().unwrap_or_default();
                        self.focus = Focus::List;
                        self.mode = Mode::Note(i, cur);
                    }
                }
                KeyCode::Backspace | KeyCode::Char('-') => {
                    self.focus = Focus::List;
                    self.go_parent();
                }
                KeyCode::Enter => {
                    if !in_preview && self.on_parent_row() {
                        self.go_parent();
                    } else if !in_preview && self.selected_dir().is_some() {
                        self.toggle_dir();
                    } else if let Some(exit) = self.open_selected() {
                        return Ok(exit);
                    }
                }
                _ => {}
            }
        }
    }

    // ------------------------------------------------------------ 그리기

    fn draw(&mut self, f: &mut Frame) {
        self.prune_changes();
        let [header, body, footer] = Layout::vertical([Constraint::Length(1), Constraint::Min(1), Constraint::Length(1)]).areas(f.area());
        self.page = body.height as usize;

        self.draw_header(f, header);

        // 폭이 좁으면 미리보기를 접는다.
        let show_preview = self.preview && body.width >= MIN_WIDTH_FOR_PREVIEW;
        self.preview_visible = show_preview;
        if !show_preview {
            self.focus = Focus::List;
        }
        let (list_area, preview_area) = if show_preview {
            let [l, r] = Layout::horizontal([Constraint::Length(self.list_width(body)), Constraint::Min(0)]).areas(body);
            (l, Some(r))
        } else {
            (body, None)
        };
        self.body_area = body;
        self.list_area = list_area;
        self.preview_area = preview_area;

        self.draw_list(f, list_area);
        if let Some(area) = preview_area {
            self.draw_preview(f, area);
        }
        self.draw_footer(f, footer, show_preview);
    }

    fn draw_header(&self, f: &mut Frame, area: Rect) {
        let tab_style = |active: bool| {
            if active {
                Style::default().fg(Color::Indexed(110)).add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
            } else {
                Style::default().fg(Color::Indexed(245))
            }
        };
        let n_stash = self.stash.borrow().entries.len();
        let where_ = if self.tab == Tab::Local {
            crate::hangul::compose(&crate::stash::shorten_home(&self.root.to_string_lossy()))
        } else {
            String::new()
        };
        let title = Line::from(vec![
            Span::styled(" mdview ", Style::default().fg(Color::Indexed(231)).bg(Color::Indexed(24)).add_modifier(Modifier::BOLD)),
            Span::raw("  "),
            Span::styled("Local", tab_style(self.tab == Tab::Local)),
            Span::raw("   "),
            Span::styled(format!("Stashed ({n_stash})"), tab_style(self.tab == Tab::Stashed)),
            Span::styled(format!("   {where_}"), Style::default().fg(Color::Indexed(245))),
        ]);
        f.render_widget(Paragraph::new(title), area);
    }

    fn draw_list(&mut self, f: &mut Frame, area: Rect) {
        let dim = Style::default().fg(Color::Indexed(245));
        let name_style = Style::default().fg(Color::Indexed(252));
        let dir_style = Style::default().fg(Color::Indexed(110));
        let change_style = Style::default().fg(Color::Indexed(110));
        let items: Vec<ListItem> = self
            .rows
            .iter()
            .map(|row| match row {
                Row::Dir { path, depth, open, files } => {
                    let name = path.file_name().map(|n| crate::hangul::compose(&n.to_string_lossy())).unwrap_or_default();
                    let mark = if *open { "▾ " } else { "▸ " };
                    let mut spans = vec![
                        Span::raw("  ".repeat(*depth)),
                        Span::styled(format!("{mark}{name}/"), dir_style),
                        Span::styled(format!("  {files}"), dim),
                    ];
                    // 접혀 있어 안이 안 보이는 폴더는, 안에서 무언가 바뀌었음을 점으로 알린다.
                    if !*open && self.changed.keys().any(|c| c.starts_with(path)) {
                        spans.push(Span::styled("  ●", change_style));
                    }
                    Line::from(spans)
                }
                Row::Item { idx, depth } => self.item_line(*idx, *depth, name_style, dim),
                Row::Parent => {
                    let up = self.root.parent().map(short).unwrap_or_default();
                    Line::from(vec![Span::styled("../", dir_style), Span::styled(format!("  {up}"), dim)])
                }
            })
            .map(ListItem::new)
            .collect();
        let focused = self.focus == Focus::List;
        let hl = if focused {
            Style::default().fg(Color::Indexed(110)).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Indexed(245))
        };
        let list = List::new(items).highlight_style(hl).highlight_symbol(if focused { "▌ " } else { "│ " });
        f.render_stateful_widget(list, area, &mut self.state);
    }

    fn item_line(&self, idx: usize, depth: usize, name_style: Style, dim: Style) -> Line<'static> {
        match self.tab {
            Tab::Local => {
                let p = &self.files[idx];
                let name = p.file_name().map(|n| crate::hangul::compose(&n.to_string_lossy())).unwrap_or_default();
                let mut spans = vec![Span::raw("  ".repeat(depth)), Span::styled(name, name_style)];
                // 평면 보기에서는 어느 디렉터리인지 뒤에 덧붙인다.
                if self.view == View::Flat {
                    let dir = p.parent().map(|d| crate::hangul::compose(&d.to_string_lossy())).unwrap_or_default();
                    if !dir.is_empty() && dir != "." {
                        spans.push(Span::styled(format!("  {dir}/"), dim));
                    }
                }
                if let Some((kind, _)) = self.changed.get(p) {
                    let label = match kind {
                        Change::Added => "  ● 새 파일",
                        Change::Modified => "  ● 변경됨",
                    };
                    spans.push(Span::styled(label, Style::default().fg(Color::Indexed(110))));
                }
                Line::from(spans)
            }
            Tab::Stashed => {
                let st = self.stash.borrow();
                let e = &st.entries[idx];
                let mut spans = vec![Span::styled(e.display_name(), name_style)];
                let dir = e.display_dir();
                if !dir.is_empty() {
                    spans.push(Span::styled(format!("  {dir}/"), dim));
                }
                if let Some(n) = &e.note {
                    spans.push(Span::styled(format!("  — {n}"), Style::default().fg(Color::Indexed(66))));
                }
                Line::from(spans)
            }
        }
    }

    fn draw_preview(&mut self, f: &mut Frame, area: Rect) {
        let focused = self.focus == Focus::Preview || self.dragging;
        // 포커스가 들어오면 경계선을 또렷하게 해서 어느 창을 움직이는지 보여 준다.
        let border_style = if focused {
            Style::default().fg(Color::Indexed(110))
        } else {
            Style::default().fg(Color::Indexed(240))
        };
        let block = Block::new().borders(Borders::LEFT).border_style(border_style);
        let inner = block.inner(area).inner(ratatui::layout::Margin { horizontal: 1, vertical: 0 });
        f.render_widget(block, area);
        self.preview_height = inner.height as usize;
        let scroll = self.preview_scroll;
        let height = inner.height as usize;
        let lines = self.preview_lines(inner.width as usize);
        let shown: Vec<Line<'static>> = lines.iter().skip(scroll).take(height).cloned().collect();
        f.render_widget(Paragraph::new(shown), inner);
    }

    /// 선택된 항목의 미리보기 줄. 같은 (경로, 폭)이면 캐시를 쓴다.
    fn preview_lines(&mut self, width: usize) -> &[Line<'static>] {
        let dim = Style::default().fg(Color::Indexed(245));
        let selected = self.selected_path();
        let key = match &selected {
            Some(Source::File(p)) => (p.to_string_lossy().into_owned(), width),
            Some(Source::Url(u)) => (u.clone(), width),
            _ => match self.selected_dir() {
                Some(d) => (format!("<dir>{}", d.display()), width),
                None if self.on_parent_row() => (format!("<parent>{}", self.root.display()), width),
                None => (String::from("<none>"), width),
            },
        };
        // 파일 표식(수정 시각·크기)이 바뀌면 같은 문서라도 다시 그린다.
        let stamp = match &selected {
            Some(Source::File(p)) => source::stamp_of(p),
            _ => None,
        };
        let same_doc = self.cache.as_ref().is_some_and(|c| c.key == key);
        let fresh = same_doc && self.cache.as_ref().is_some_and(|c| c.stamp == stamp);
        if !fresh {
            let lines = self.render_preview(&key.0, width, dim);
            // 다른 문서를 고르면 맨 위부터. 같은 문서가 고쳐진 것이면 보던 자리를 지킨다.
            if !same_doc {
                self.preview_scroll = 0;
            }
            self.cache = Some(PreviewCache { key, stamp, lines });
        }
        let len = self.cache.as_ref().map_or(0, |c| c.lines.len());
        self.preview_len = len;
        // 내용이 짧아졌으면 스크롤을 끝에 맞춘다.
        self.preview_scroll = self.preview_scroll.min(len.saturating_sub(self.preview_height));
        &self.cache.as_ref().unwrap().lines
    }

    fn render_preview(&self, key: &str, width: usize, dim: Style) -> Vec<Line<'static>> {
        let note = |s: &str| vec![Line::from(Span::styled(s.to_string(), dim))];
        if width < 8 {
            return Vec::new();
        }
        if let Some(d) = key.strip_prefix("<dir>") {
            let n = self.rows.iter().filter(|r| matches!(r, Row::Dir { path, .. } if path == &PathBuf::from(d))).count();
            let _ = n;
            return note(&format!("{d}/  — 디렉터리 (Enter 또는 h/l 로 접고 펴기)"));
        }
        if key == "<none>" {
            return note("선택된 항목이 없습니다");
        }
        if let Some(root) = key.strip_prefix("<parent>") {
            let up = Path::new(root).parent().map(short).unwrap_or_default();
            return note(&format!("../  — 상위 폴더 {up} 로 이동 (Enter, Backspace, -)"));
        }
        if crate::source::is_url(key) {
            return note("원격 문서입니다. Enter 로 열면 내려받습니다.");
        }
        let path = PathBuf::from(key);
        match std::fs::metadata(&path) {
            Ok(m) if m.len() > MAX_PREVIEW_BYTES => {
                return note(&format!("파일이 큽니다 ({} KB). Enter 로 열어 보세요.", m.len() / 1024));
            }
            Err(e) => return note(&format!("읽을 수 없습니다: {e}")),
            _ => {}
        }
        let Ok(bytes) = std::fs::read(&path) else {
            return note("읽을 수 없습니다");
        };
        let text = crate::hangul::compose(&String::from_utf8_lossy(&bytes));
        let mut out: Vec<Line<'static>> = crate::render::render(&text, &self.theme, width).iter().map(tui_convert::line).collect();
        if out.is_empty() {
            out = note("(빈 문서)");
        }
        out
    }

    fn draw_footer(&mut self, f: &mut Frame, area: Rect, show_preview: bool) {
        let dim = Style::default().fg(Color::Indexed(245));
        if self.status.as_ref().is_some_and(|(_, t)| t.elapsed() > STATUS_TTL) {
            self.status = None;
        }
        let files = self.rows.iter().filter(|r| r.item_index().is_some()).count();
        let text = match &self.mode {
            Mode::Filter => format!("/{}", self.filter),
            Mode::Note(_, t) => format!("note: {t}▏  (Enter save, Esc cancel)"),
            Mode::GoTo(t) => match &self.status {
                Some((msg, _)) => format!("cd: {t}▏  {msg}"),
                None => format!("cd: {t}▏  (Tab complete, Enter go, Esc cancel)"),
            },
            Mode::Normal => {
                if let Some((msg, _)) = &self.status {
                    msg.clone()
                } else if self.scanning {
                    format!("{} 훑는 중…", short(&self.root))
                } else if files == 0 {
                    match self.tab {
                        Tab::Local => "no markdown files found  Tab stashed  q quit".to_string(),
                        Tab::Stashed => "stash is empty — press s on a file, or: mdview stash FILE|URL  Tab local  q quit".to_string(),
                    }
                } else if self.focus == Focus::Preview {
                    // 미리보기 안에서는 그쪽 조작만 안내한다.
                    let max = self.preview_len.saturating_sub(self.preview_height);
                    let pct = (self.preview_scroll * 100).checked_div(max).unwrap_or(100);
                    format!("preview {pct:>3}%  j/k gg/G ^d^u^f^b scroll  h/Esc back to list  Enter open  q quit")
                } else {
                    let hint = if self.preview && !show_preview { "  (창이 좁아 미리보기 접힘)" } else { "" };
                    match self.tab {
                        Tab::Local => format!("{files} files  j/k gg/G  h/l fold·preview  H/L fold all  Enter open  -/⌫ up  c cd  v view  p preview  s stash  / filter  q quit{hint}"),
                        Tab::Stashed => format!("{files} stashed  j/k gg/G  l preview  Enter open  x remove  m note  / filter  Tab local  q quit{hint}"),
                    }
                }
            }
        };
        f.render_widget(Paragraph::new(Span::styled(format!(" {text}"), dim)), area);
    }
}

enum Pos {
    First,
    Last,
}

/// 목록이 바뀌어도 커서를 되찾기 위한 표식.
enum Anchor {
    Parent,
    Dir(PathBuf),
    File(PathBuf),
}

/// 화면에 보여 줄 짧은 경로(`~` 줄임, 한글 자소 결합).
fn short(p: &Path) -> String {
    crate::hangul::compose(&crate::stash::shorten_home(&p.to_string_lossy()))
}

/// 입력한 경로를 폴더로 푼다. `~`는 홈, 상대 경로는 `root` 기준.
fn resolve_dir(root: &Path, input: &str) -> Result<PathBuf, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("경로를 입력하세요".into());
    }
    let path = expand_tilde(root, input);
    match path.canonicalize() {
        Ok(p) if p.is_dir() => Ok(p),
        Ok(_) => Err(format!("폴더가 아닙니다: {}", short(&path))),
        Err(_) => Err(format!("없는 경로입니다: {}", short(&path))),
    }
}

/// `~`를 홈으로 바꾸고 상대 경로는 `root`에 붙인다.
fn expand_tilde(root: &Path, input: &str) -> PathBuf {
    if let Some(rest) = input.strip_prefix('~')
        && (rest.is_empty() || rest.starts_with('/'))
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home).join(rest.trim_start_matches('/'));
    }
    root.join(input)
}

/// 입력 중인 경로의 마지막 조각을 하위 폴더 이름으로 채운다.
/// 하나만 맞으면 `/`까지 붙이고, 여럿이면 공통 접두어까지만. 더 채울 것이 없으면 `None`.
fn complete_dir(root: &Path, input: &str) -> Option<String> {
    let (head, partial) = match input.rfind('/') {
        Some(i) => (&input[..=i], &input[i + 1..]),
        None => ("", input),
    };
    let dir = if head.is_empty() { root.to_path_buf() } else { expand_tilde(root, head) };
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()) || e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.starts_with(partial) && (partial.starts_with('.') || !n.starts_with('.')))
        .collect();
    names.sort();
    let done = match names.as_slice() {
        [] => return None,
        [one] => format!("{one}/"),
        many => common_prefix(many),
    };
    (done != partial).then(|| format!("{head}{done}"))
}

fn common_prefix(names: &[String]) -> String {
    let first = &names[0];
    let len = first
        .char_indices()
        .map(|(i, c)| i + c.len_utf8())
        .take_while(|&end| names.iter().all(|n| n.get(..end) == first.get(..end)))
        .last()
        .unwrap_or(0);
    first[..len].to_string()
}

fn rect_contains(r: Rect, x: u16, y: u16) -> bool {
    x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picker(files: &[&str]) -> Picker {
        let stash = Stash::load_from(std::env::temp_dir().join(format!("mdview-picker-{}.json", std::process::id())));
        Picker::new(
            PathBuf::from("/"),
            files.iter().map(PathBuf::from).collect(),
            Rc::new(RefCell::new(stash)),
            Theme::notty(),
        )
    }

    /// 선택된 행을 사람이 읽을 수 있는 형태로.
    fn at(p: &Picker) -> String {
        match p.state.selected().and_then(|s| p.rows.get(s)) {
            Some(Row::Dir { path, open, .. }) => format!("{}/{}", path.display(), if *open { "" } else { "(접힘)" }),
            Some(Row::Item { idx, .. }) => p.files[*idx].display().to_string(),
            Some(Row::Parent) => "../".into(),
            None => "(없음)".into(),
        }
    }

    #[test]
    fn tree_is_the_default_view_and_starts_on_a_file() {
        let p = picker(&["README.md", "docs/a.md", "docs/sub/b.md"]);
        assert_eq!(p.view, View::Tree);
        // 첫 행은 docs/ 디렉터리이지만 커서는 첫 파일에 놓인다.
        assert!(matches!(p.rows[0], Row::Dir { .. }));
        assert_eq!(at(&p), "docs/sub/b.md");
    }

    #[test]
    fn motions_clamp_at_both_ends() {
        let mut p = picker(&["a.md", "b.md", "c.md"]);
        p.goto(Pos::First);
        p.move_sel(-50);
        assert_eq!(at(&p), "a.md", "위로 넘쳐도 첫 행에 멈춘다");
        p.move_sel(50);
        assert_eq!(at(&p), "c.md", "아래로 넘쳐도 마지막 행에 멈춘다");
        p.goto(Pos::Last);
        assert_eq!(at(&p), "c.md");
        p.goto(Pos::First);
        assert_eq!(at(&p), "a.md");
    }

    #[test]
    fn motions_on_empty_list_do_not_panic() {
        let mut p = picker(&[]);
        p.move_sel(1);
        p.move_sel(-1);
        p.goto(Pos::First);
        p.goto(Pos::Last);
        assert_eq!(at(&p), "(없음)");
    }

    #[test]
    fn toggling_view_keeps_the_selected_file() {
        let mut p = picker(&["README.md", "docs/a.md", "docs/sub/b.md"]);
        p.goto(Pos::Last);
        let before = at(&p);
        p.toggle_view();
        assert_eq!(p.view, View::Flat);
        assert_eq!(at(&p), before, "평면으로 바꿔도 같은 파일을 가리킨다");
        p.toggle_view();
        assert_eq!(p.view, View::Tree);
        assert_eq!(at(&p), before, "트리로 돌아와도 마찬가지");
    }

    #[test]
    fn flat_view_has_no_directory_rows() {
        let mut p = picker(&["README.md", "docs/a.md"]);
        p.toggle_view();
        assert!(p.rows.iter().all(|r| r.item_index().is_some()));
        assert_eq!(p.rows.len(), 2);
    }

    #[test]
    fn collapsing_a_directory_hides_its_files() {
        let mut p = picker(&["README.md", "docs/a.md", "docs/sub/b.md"]);
        p.goto(Pos::First);
        assert_eq!(at(&p), "docs/");
        p.toggle_dir();
        assert_eq!(at(&p), "docs/(접힘)");
        assert_eq!(p.rows.len(), 2, "docs/ 와 README.md 만 남는다");
        p.toggle_dir();
        assert!(p.rows.len() > 2);
    }

    #[test]
    fn filter_reveals_matches_inside_collapsed_directories() {
        let mut p = picker(&["README.md", "docs/guide.md"]);
        p.goto(Pos::First);
        p.toggle_dir();
        assert_eq!(p.rows.len(), 2);
        p.filter = "guide".into();
        p.rebuild();
        assert!(p.rows.iter().any(|r| r.item_index() == Some(1)), "접혀 있어도 필터 결과는 보인다");
    }

    #[test]
    fn h_on_a_file_jumps_to_its_parent_directory() {
        let mut p = picker(&["docs/sub/b.md"]);
        assert_eq!(at(&p), "docs/sub/b.md");
        p.collapse_or_parent();
        assert_eq!(at(&p), "docs/sub/");
    }

    #[test]
    fn h_on_a_collapsed_directory_goes_to_parent_not_expand() {
        let mut p = picker(&["docs/sub/b.md", "docs/a.md"]);
        // docs/sub/ 로 가서 접는다
        let pos = p.rows.iter().position(|r| matches!(r, Row::Dir { path, .. } if path == &PathBuf::from("docs/sub"))).unwrap();
        p.state.select(Some(pos));
        p.collapse_or_parent();
        assert_eq!(at(&p), "docs/sub/(접힘)");
        // 접힌 디렉터리에서 h 를 다시 누르면 펴지지 않고 부모로 간다
        p.collapse_or_parent();
        assert_eq!(at(&p), "docs/");
        assert!(p.collapsed.contains(&PathBuf::from("docs/sub")), "접힘 상태는 그대로");
    }

    #[test]
    fn l_expands_a_collapsed_directory_then_steps_inside() {
        let mut p = picker(&["docs/a.md", "top.md"]);
        p.goto(Pos::First);
        p.toggle_dir();
        assert_eq!(at(&p), "docs/(접힘)");
        p.expand_or_enter_preview();
        assert_eq!(at(&p), "docs/", "첫 l 은 편다");
        p.expand_or_enter_preview();
        assert_eq!(at(&p), "docs/a.md", "열린 디렉터리에서 l 은 첫 하위로");
        p.preview_visible = true;
        p.expand_or_enter_preview();
        assert_eq!(p.focus, Focus::Preview, "파일 위에서 l 은 미리보기로");
    }

    #[test]
    fn collapse_all_and_expand_all_keep_context() {
        let mut p = picker(&["a/x.md", "a/b/y.md", "c/z.md", "top.md"]);
        // a/b/y.md 를 보고 있다가 전체 접기
        p.goto(Pos::First);
        while at(&p) != "a/b/y.md" {
            p.move_sel(1);
        }
        p.collapse_all();
        assert_eq!(p.rows.len(), 3, "a/, c/, top.md 만 남는다");
        assert_eq!(at(&p), "a/(접힘)", "보던 항목의 최상위 디렉터리에 선다");
        // 전체 펴기: 접힌 것이 없고 커서는 같은 행(a/)에 남는다
        p.expand_all();
        assert!(p.collapsed.is_empty());
        assert_eq!(at(&p), "a/");
        assert_eq!(p.rows.len(), 7);
    }

    #[test]
    fn expand_all_keeps_the_selected_file() {
        let mut p = picker(&["a/x.md", "a/b/y.md"]);
        p.goto(Pos::First);
        p.toggle_dir(); // a/ 접기
        p.expand_all();
        p.goto(Pos::Last);
        let before = at(&p);
        p.expand_all();
        assert_eq!(at(&p), before);
    }

    #[test]
    fn divider_drag_respects_minimum_widths() {
        let mut p = picker(&["a.md"]);
        p.body_area = Rect::new(0, 1, 100, 30);
        p.set_split_at(10);
        assert_eq!(p.list_width(p.body_area), MIN_LIST_COLS, "목록은 최소 폭 아래로 줄지 않는다");
        p.set_split_at(95);
        assert_eq!(p.list_width(p.body_area), 100 - MIN_PREVIEW_COLS, "미리보기도 최소 폭을 지킨다");
        p.set_split_at(60);
        assert_eq!(p.list_width(p.body_area), 60);
    }

    #[test]
    fn divider_drag_is_ignored_when_the_body_is_too_narrow() {
        let mut p = picker(&["a.md"]);
        p.body_area = Rect::new(0, 1, 40, 30);
        let before = p.split_pct;
        p.set_split_at(30);
        assert_eq!(p.split_pct, before);
    }

    fn mouse(kind: MouseEventKind, x: u16, y: u16) -> MouseEvent {
        MouseEvent { kind, column: x, row: y, modifiers: KeyModifiers::empty() }
    }

    #[test]
    fn dragging_the_divider_moves_the_split() {
        let mut p = picker(&["a.md"]);
        p.body_area = Rect::new(0, 1, 100, 30);
        p.list_area = Rect::new(0, 1, 42, 30);
        p.preview_area = Some(Rect::new(42, 1, 58, 30));
        p.on_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 42, 5));
        assert!(p.dragging);
        p.on_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), 70, 5));
        assert_eq!(p.list_width(p.body_area), 70);
        p.on_mouse(mouse(MouseEventKind::Up(MouseButton::Left), 70, 5));
        assert!(!p.dragging);
        // 구분선이 아닌 곳을 눌러도 끌기가 시작되지 않는다
        p.on_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 10, 5));
        assert!(!p.dragging);
    }

    #[test]
    fn clicking_a_row_selects_it_and_wheel_scrolls_the_right_pane() {
        let mut p = picker(&["a.md", "b.md", "c.md"]);
        p.body_area = Rect::new(0, 1, 100, 30);
        p.list_area = Rect::new(0, 1, 42, 30);
        p.preview_area = Some(Rect::new(42, 1, 58, 30));
        p.on_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 5, 3));
        assert_eq!(at(&p), "c.md", "세 번째 줄을 누르면 c.md");
        assert_eq!(p.focus, Focus::List);
        p.on_mouse(mouse(MouseEventKind::ScrollUp, 5, 3));
        assert_eq!(at(&p), "b.md");
        p.preview_len = 100;
        p.preview_height = 10;
        p.on_mouse(mouse(MouseEventKind::ScrollDown, 60, 3));
        assert_eq!(p.preview_scroll, 3, "미리보기 위에서 휠은 미리보기를 움직인다");
        p.on_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 60, 3));
        assert_eq!(p.focus, Focus::Preview, "미리보기를 누르면 포커스가 옮겨진다");
    }

    #[test]
    fn preview_scroll_clamps_at_both_ends() {
        let mut p = picker(&["a.md"]);
        p.preview_len = 100;
        p.preview_height = 10;
        p.scroll_preview(-5);
        assert_eq!(p.preview_scroll, 0, "맨 위에서 더 올라가지 않는다");
        p.scroll_preview(500);
        assert_eq!(p.preview_scroll, 90, "마지막 화면이 꽉 찬 위치에서 멈춘다");
        p.scroll_preview(1);
        assert_eq!(p.preview_scroll, 90);
    }

    #[test]
    fn preview_scroll_jumps_to_ends() {
        let mut p = picker(&["a.md"]);
        p.preview_len = 40;
        p.preview_height = 12;
        p.scroll_preview_to(Pos::Last);
        assert_eq!(p.preview_scroll, 28);
        p.scroll_preview_to(Pos::First);
        assert_eq!(p.preview_scroll, 0);
    }

    #[test]
    fn short_document_never_scrolls() {
        let mut p = picker(&["a.md"]);
        p.preview_len = 5;
        p.preview_height = 20;
        p.scroll_preview(10);
        assert_eq!(p.preview_scroll, 0);
        p.scroll_preview_to(Pos::Last);
        assert_eq!(p.preview_scroll, 0);
    }

    #[test]
    fn l_enters_the_preview_pane() {
        let mut p = picker(&["a.md"]);
        p.preview_visible = true;
        assert_eq!(p.focus, Focus::List);
        p.enter_preview();
        assert_eq!(p.focus, Focus::Preview);
    }

    #[test]
    fn l_turns_the_preview_on_if_it_was_off() {
        let mut p = picker(&["a.md"]);
        p.preview = false;
        p.enter_preview();
        assert!(p.preview, "꺼져 있었으면 켜고 들어간다");
        assert_eq!(p.focus, Focus::Preview);
    }

    #[test]
    fn narrow_window_refuses_to_enter_and_says_why() {
        let mut p = picker(&["a.md"]);
        p.preview = true;
        p.preview_visible = false;
        p.enter_preview();
        assert_eq!(p.focus, Focus::List, "창이 좁으면 들어가지 않는다");
        assert!(p.status.as_ref().unwrap().0.contains("좁아"));
    }

    #[test]
    fn choosing_another_document_rewinds_the_preview() {
        let dir = std::env::temp_dir().join(format!("mdview-rewind-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (name, body) in [("a.md", "# A

본문
"), ("b.md", "# B

본문
")] {
            std::fs::write(dir.join(name), body).unwrap();
        }
        let mut p = picker(&["a.md", "b.md"]);
        p.root = dir.clone();
        p.goto(Pos::First);
        p.preview_lines(40); // a.md 를 캐시에 올린다
        p.preview_scroll = 7;
        p.goto(Pos::Last);
        p.preview_lines(40); // b.md 로 바뀌면 처음부터
        assert_eq!(p.preview_scroll, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn preview_of_a_missing_file_reports_the_error() {
        let p = picker(&["gone.md"]);
        let lines = p.render_preview("/tmp/definitely-not-here.md", 40, Style::default());
        assert!(lines[0].spans[0].content.contains("읽을 수 없습니다"));
    }

    #[test]
    fn preview_of_a_url_does_not_fetch() {
        let p = picker(&[]);
        let lines = p.render_preview("https://example.com/a.md", 40, Style::default());
        assert!(lines[0].spans[0].content.contains("Enter"));
    }

    #[test]
    fn preview_renders_markdown_headings() {
        let dir = std::env::temp_dir().join(format!("mdview-preview-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("x.md");
        std::fs::write(&file, "# 제목\n\n본문\n").unwrap();
        let p = picker(&["x.md"]);
        let lines = p.render_preview(&file.to_string_lossy(), 30, Style::default());
        let text: Vec<String> = lines.iter().map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect()).collect();
        assert!(text.iter().any(|l| l.contains("제목")));
        assert!(text.iter().any(|l| l.contains('━')), "제목 밑줄까지 그려진다");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ------------------------------------------------------------ 상위 폴더 · 자동 갱신

    fn st(secs: u64, len: u64) -> Stamp {
        (Some(std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(secs)), len)
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mdview-picker-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// 실제 폴더를 훑어 만든 브라우저(감시기 없음).
    fn picker_at(root: &Path) -> Picker {
        let scan = crate::source::scan_markdown_files(root);
        let stash = Stash::load_from(std::env::temp_dir().join(format!("mdview-picker-{}.json", std::process::id())));
        let mut p = Picker::new(root.to_path_buf(), scan.iter().map(|(f, _)| f.clone()).collect(), Rc::new(RefCell::new(stash)), Theme::notty());
        p.stamps = scan;
        p.baseline = true;
        p
    }

    /// base/{proj/a.md, proj/sub/b.md, other/c.md, top.md}
    fn project_tree(name: &str) -> PathBuf {
        let base = tmp(name);
        for f in ["proj/a.md", "proj/sub/b.md", "other/c.md", "top.md"] {
            let path = base.join(f);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "# x\n").unwrap();
        }
        base
    }

    #[test]
    fn parent_row_is_on_top_but_hidden_while_filtering() {
        let dir = tmp("parentrow");
        std::fs::write(dir.join("a.md"), "# a").unwrap();
        let mut p = picker_at(&dir);
        assert!(matches!(p.rows[0], Row::Parent));
        assert_eq!(at(&p), "a.md", "처음 커서는 `../` 가 아니라 첫 파일에 선다");
        p.filter = "a".into();
        p.rebuild();
        assert!(!p.rows.iter().any(|r| matches!(r, Row::Parent)));
        assert_eq!(at(&p), "a.md", "`../` 가 사라져도 같은 파일을 가리킨다");
        p.filter.clear();
        p.rebuild();
        assert_eq!(at(&p), "a.md", "다시 생겨도 마찬가지");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn filesystem_root_has_no_parent_row() {
        let mut p = picker(&["a.md"]);
        assert!(!p.rows.iter().any(|r| matches!(r, Row::Parent)));
        p.go_parent();
        assert_eq!(p.root, PathBuf::from("/"));
        assert!(p.status.as_ref().unwrap().0.contains("최상위"));
    }

    #[test]
    fn going_up_lands_on_the_folder_we_left_with_siblings_folded() {
        let base = project_tree("up");
        let mut p = picker_at(&base.join("proj"));
        p.collapsed.insert(PathBuf::from("sub"));
        p.go_parent();
        assert_eq!(p.root, base);
        assert_eq!(at(&p), "proj/", "올라오기 전 폴더에 커서가 선다");
        assert!(p.collapsed.contains(&PathBuf::from("other")), "형제 폴더는 접어 둔다");
        assert!(!p.collapsed.contains(&PathBuf::from("proj")), "왔던 폴더는 펴 둔다");
        assert!(p.collapsed.contains(&PathBuf::from("proj/sub")), "접어 둔 하위 폴더는 새 경로로 유지");
        assert!(p.files.contains(&PathBuf::from("top.md")));
        assert!(p.changed.is_empty(), "새 루트의 첫 결과는 모두 '새 파일'로 표시하지 않는다");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn enter_on_parent_row_goes_up() {
        let base = project_tree("enterup");
        let mut p = picker_at(&base.join("proj"));
        p.goto(Pos::First);
        assert_eq!(at(&p), "../");
        assert!(p.on_parent_row());
        p.go_parent();
        assert_eq!(p.root, base);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn going_up_with_the_watcher_scans_in_the_background() {
        let base = project_tree("upwatch");
        let proj = base.join("proj");
        let mut p = picker_at(&proj);
        p.start_watching(crate::source::scan_markdown_files(&proj));
        p.go_parent();
        assert!(p.scanning, "넓은 폴더에서도 화면이 멈추지 않도록 결과를 기다린다");
        let deadline = Instant::now() + Duration::from_secs(6);
        while p.scanning && Instant::now() < deadline {
            if let Some(snap) = p.watcher.as_ref().and_then(Watcher::latest) {
                p.apply_snapshot(snap);
            }
            std::thread::sleep(Duration::from_millis(30));
        }
        assert!(!p.scanning);
        assert_eq!(at(&p), "proj/");
        assert_eq!(p.files.len(), 4);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn snapshot_marks_new_and_changed_files_and_keeps_the_cursor() {
        let mut p = picker(&["a.md", "b.md"]);
        p.stamps = vec![(PathBuf::from("a.md"), st(1, 1)), (PathBuf::from("b.md"), st(1, 1))];
        p.baseline = true;
        p.goto(Pos::Last);
        assert_eq!(at(&p), "b.md");
        p.apply_snapshot(Snapshot {
            root: PathBuf::from("/"),
            files: vec![(PathBuf::from("0new.md"), st(2, 1)), (PathBuf::from("a.md"), st(1, 1)), (PathBuf::from("b.md"), st(3, 9))],
            done: true,
        });
        assert_eq!(at(&p), "b.md", "위에 파일이 끼어들어도 보던 파일을 계속 가리킨다");
        assert_eq!(p.changed.get(Path::new("0new.md")).map(|c| c.0), Some(Change::Added));
        assert_eq!(p.changed.get(Path::new("b.md")).map(|c| c.0), Some(Change::Modified));
        assert!(!p.changed.contains_key(Path::new("a.md")));
        assert_eq!(p.status.as_ref().unwrap().0, "목록 갱신: 추가 1, 변경 1");
    }

    #[test]
    fn snapshot_for_an_old_root_is_ignored() {
        let mut p = picker(&["a.md"]);
        p.apply_snapshot(Snapshot { root: PathBuf::from("/elsewhere"), files: vec![(PathBuf::from("x.md"), st(1, 1))], done: true });
        assert_eq!(p.files, vec![PathBuf::from("a.md")]);
    }

    #[test]
    fn deleting_the_selected_file_keeps_the_cursor_in_range() {
        let mut p = picker(&["a.md", "b.md"]);
        p.stamps = vec![(PathBuf::from("a.md"), st(1, 1)), (PathBuf::from("b.md"), st(1, 1))];
        p.baseline = true;
        p.goto(Pos::Last);
        p.apply_snapshot(Snapshot { root: PathBuf::from("/"), files: vec![(PathBuf::from("a.md"), st(1, 1))], done: true });
        assert_eq!(at(&p), "a.md");
        assert_eq!(p.status.as_ref().unwrap().0, "삭제됨: b.md");
    }

    #[test]
    fn change_marks_fade_after_a_while() {
        let mut p = picker(&["a.md", "b.md"]);
        let old = Instant::now().checked_sub(CHANGE_TTL + Duration::from_secs(1)).unwrap();
        p.changed.insert(PathBuf::from("a.md"), (Change::Added, old));
        p.changed.insert(PathBuf::from("b.md"), (Change::Modified, Instant::now()));
        p.prune_changes();
        assert!(!p.changed.contains_key(Path::new("a.md")));
        assert!(p.changed.contains_key(Path::new("b.md")));
    }

    #[test]
    fn preview_picks_up_edits_without_losing_the_scroll_position() {
        let dir = tmp("reload");
        let file = dir.join("doc.md");
        let body: String = (1..=40).map(|i| format!("줄 {i}\n\n")).collect();
        std::fs::write(&file, &body).unwrap();
        let mut p = picker_at(&dir);
        assert_eq!(at(&p), "doc.md");
        p.preview_height = 5;
        p.preview_lines(40);
        p.preview_scroll = 6;
        std::fs::write(&file, format!("{body}추가된 끝 줄\n")).unwrap();
        let text: Vec<String> = p.preview_lines(40).iter().map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect()).collect();
        assert!(text.iter().any(|l| l.contains("추가된 끝 줄")), "고친 내용이 바로 보인다");
        assert_eq!(p.preview_scroll, 6, "보던 위치는 그대로");
        let _ = std::fs::remove_dir_all(&dir);
    }
    // ------------------------------------------------------------ 경로 입력으로 이동

    #[test]
    fn resolve_dir_takes_absolute_paths_as_is() {
        let dir = tmp("resolve-abs");
        assert_eq!(resolve_dir(Path::new("/"), &dir.to_string_lossy()).unwrap(), dir.canonicalize().unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_dir_reads_relative_paths_from_the_current_root() {
        let base = project_tree("resolve-rel");
        assert_eq!(resolve_dir(&base, "proj/sub").unwrap(), base.join("proj/sub").canonicalize().unwrap());
        assert_eq!(resolve_dir(&base.join("proj"), "..").unwrap(), base.canonicalize().unwrap());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn resolve_dir_expands_tilde_to_home() {
        let home = PathBuf::from(std::env::var_os("HOME").unwrap()).canonicalize().unwrap();
        assert_eq!(resolve_dir(Path::new("/"), "~").unwrap(), home);
        assert_eq!(resolve_dir(Path::new("/"), "~/").unwrap(), home);
    }

    #[test]
    fn resolve_dir_rejects_missing_paths_and_files() {
        let base = project_tree("resolve-bad");
        assert!(resolve_dir(&base, "nope").unwrap_err().contains("없"), "없는 경로");
        assert!(resolve_dir(&base, "top.md").unwrap_err().contains("폴더가 아닙니다"), "파일");
        assert!(resolve_dir(&base, "").is_err(), "빈 입력");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn complete_dir_fills_a_unique_match_and_adds_a_slash() {
        let base = project_tree("complete-one");
        assert_eq!(complete_dir(&base, "pr").as_deref(), Some("proj/"));
        assert_eq!(complete_dir(&base, "proj/s").as_deref(), Some("proj/sub/"));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn complete_dir_fills_only_the_common_prefix_when_ambiguous() {
        let base = tmp("complete-many");
        std::fs::create_dir_all(base.join("alpha-one")).unwrap();
        std::fs::create_dir_all(base.join("alpha-two")).unwrap();
        std::fs::write(base.join("alpha-file.md"), "x").unwrap();
        assert_eq!(complete_dir(&base, "al").as_deref(), Some("alpha-"), "파일은 후보에서 빼고 공통 접두어까지만");
        assert_eq!(complete_dir(&base, "alpha-").as_deref(), None, "더 채울 것이 없다");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn complete_dir_returns_none_when_nothing_matches() {
        let base = project_tree("complete-none");
        assert_eq!(complete_dir(&base, "zzz"), None);
        assert_eq!(complete_dir(&base, "nope/x"), None);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn complete_dir_keeps_the_typed_prefix_for_absolute_and_tilde_paths() {
        let base = project_tree("complete-abs");
        let typed = format!("{}/pr", base.display());
        assert_eq!(complete_dir(Path::new("/"), &typed).as_deref(), Some(format!("{}/proj/", base.display()).as_str()));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn go_to_moves_the_root_and_lists_its_files() {
        let base = project_tree("goto");
        let mut p = picker_at(&base);
        p.collapsed.insert(PathBuf::from("other"));
        p.mode = Mode::GoTo("proj/sub".into());
        p.go_to();
        assert!(matches!(p.mode, Mode::Normal));
        assert_eq!(p.root, base.join("proj/sub").canonicalize().unwrap());
        assert!(p.collapsed.is_empty(), "접힘 상태는 새 루트에서 초기화");
        assert!(p.files.iter().all(|f| f.extension().is_some()), "새 루트의 파일만 보인다");
        assert!(!p.files.contains(&PathBuf::from("top.md")));
        assert!(p.status.as_ref().unwrap().0.contains("sub"));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn go_to_a_bad_path_reports_and_keeps_the_prompt_open() {
        let base = project_tree("goto-bad");
        let mut p = picker_at(&base);
        p.mode = Mode::GoTo("nope".into());
        p.go_to();
        assert!(matches!(&p.mode, Mode::GoTo(t) if t == "nope"), "입력창은 그대로");
        assert_eq!(p.root, base);
        assert!(p.status.as_ref().unwrap().0.contains("없"));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn go_to_is_ignored_on_the_stashed_tab() {
        let base = project_tree("goto-stash");
        let mut p = picker_at(&base);
        p.tab = Tab::Stashed;
        p.mode = Mode::GoTo("proj".into());
        p.go_to();
        assert_eq!(p.root, base);
        let _ = std::fs::remove_dir_all(&base);
    }

    // ------------------------------------------------------------ 부분 스냅숏 · 즉시 표시

    #[test]
    fn partial_snapshots_fill_the_list_but_keep_scanning() {
        let base = project_tree("partial");
        let mut p = picker_at(&base.join("proj"));
        p.start_watching(crate::source::scan_markdown_files(&base.join("proj")));
        p.go_parent();
        assert!(p.scanning);
        p.apply_snapshot(Snapshot { root: base.clone(), files: vec![(PathBuf::from("top.md"), st(1, 1))], done: false });
        assert!(p.scanning, "부분 결과로는 훑기가 끝나지 않는다");
        assert!(p.files.contains(&PathBuf::from("top.md")), "부분 결과도 바로 보인다");
        p.apply_snapshot(Snapshot { root: base.clone(), files: crate::source::scan_markdown_files(&base), done: true });
        assert!(!p.scanning);
        assert_eq!(p.files.len(), 4);
        assert!(p.changed.is_empty(), "새 루트의 첫 결과는 변경 표시를 붙이지 않는다");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn going_up_shows_the_subtree_we_came_from_before_the_scan_returns() {
        let base = project_tree("seed");
        let proj = base.join("proj");
        let mut p = picker_at(&proj);
        p.start_watching(crate::source::scan_markdown_files(&proj));
        p.go_parent();
        assert!(p.scanning);
        assert_eq!(p.files, vec![PathBuf::from("proj/a.md"), PathBuf::from("proj/sub/b.md")], "이미 알던 하위 트리는 즉시 보여 준다");
        assert_eq!(at(&p), "proj/", "커서는 올라오기 전 폴더에");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn cursor_lands_on_the_folder_we_left_even_when_it_arrives_in_a_partial_snapshot() {
        let base = project_tree("seed-partial");
        let mut p = picker_at(&base.join("proj"));
        p.start_watching(crate::source::scan_markdown_files(&base.join("proj")));
        p.go_parent();
        p.apply_snapshot(Snapshot { root: base.clone(), files: vec![(PathBuf::from("other/c.md"), st(1, 1)), (PathBuf::from("proj/a.md"), st(1, 1))], done: false });
        assert_eq!(at(&p), "proj/");
        assert!(p.collapsed.contains(&PathBuf::from("other")));
        p.apply_snapshot(Snapshot { root: base.clone(), files: crate::source::scan_markdown_files(&base), done: true });
        assert_eq!(at(&p), "proj/", "완료본이 와도 커서는 그대로");
        let _ = std::fs::remove_dir_all(&base);
    }
}
