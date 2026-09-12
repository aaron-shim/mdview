//! 마크다운 파일 브라우저 TUI: Local 탭(현재 디렉터리)과 Stashed 탭(즐겨찾기).
//!
//! 목록은 평면/트리 두 가지로 볼 수 있고(`v`), 오른쪽에 미리보기 창을 붙일 수
//! 있다(`p`). 이동은 vi 키(j/k, Ctrl-d/u/f/b, gg, G)를 따르고, 트리는 h/l 로
//! 접고 편다(H/L 은 전체). 미리보기 창 폭은 구분선을 마우스로 끌어 바꾼다.

use crate::output::tree::{self, Row};
use crate::output::tui_convert;
use crate::source::Source;
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
use std::collections::HashSet;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Local,
    Stashed,
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
}

/// 미리보기로 그려 둔 내용. 같은 (경로, 폭)이면 다시 렌더링하지 않는다.
struct PreviewCache {
    key: (String, usize),
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
        if self.rows.is_empty() {
            self.state.select(None);
        } else {
            let sel = self.state.selected().unwrap_or(0).min(self.rows.len() - 1);
            self.state.select(Some(sel));
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
        let top = self.state.selected().and_then(|s| self.rows.get(s)).map(|r| match r {
            Row::Dir { path, .. } => path.clone(),
            Row::Item { idx, .. } => self.files[*idx].clone(),
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
                KeyCode::Enter => {
                    if !in_preview && self.selected_dir().is_some() {
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
        let items: Vec<ListItem> = self
            .rows
            .iter()
            .map(|row| match row {
                Row::Dir { path, depth, open, files } => {
                    let name = path.file_name().map(|n| crate::hangul::compose(&n.to_string_lossy())).unwrap_or_default();
                    let mark = if *open { "▾ " } else { "▸ " };
                    Line::from(vec![
                        Span::raw("  ".repeat(*depth)),
                        Span::styled(format!("{mark}{name}/"), dir_style),
                        Span::styled(format!("  {files}"), dim),
                    ])
                }
                Row::Item { idx, depth } => self.item_line(*idx, *depth, name_style, dim),
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
        let key = match self.selected_path() {
            Some(Source::File(p)) => (p.to_string_lossy().into_owned(), width),
            Some(Source::Url(u)) => (u, width),
            _ => match self.selected_dir() {
                Some(d) => (format!("<dir>{}", d.display()), width),
                None => (String::from("<none>"), width),
            },
        };
        if self.cache.as_ref().is_none_or(|c| c.key != key) {
            // 다른 문서를 고르면 맨 위부터 보여 준다.
            let lines = self.render_preview(&key.0, width, dim);
            self.preview_scroll = 0;
            self.cache = Some(PreviewCache { key, lines });
        }
        let lines = &self.cache.as_ref().unwrap().lines;
        self.preview_len = lines.len();
        lines
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
            Mode::Normal => {
                if let Some((msg, _)) = &self.status {
                    msg.clone()
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
                        Tab::Local => format!("{files} files  j/k gg/G  h/l fold·preview  H/L fold all  Enter open  v view  p preview  s stash  / filter  q quit{hint}"),
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

fn rect_contains(r: Rect, x: u16, y: u16) -> bool {
    x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picker(files: &[&str]) -> Picker {
        let stash = Stash::load_from(std::env::temp_dir().join(format!("mdview-picker-{}.json", std::process::id())));
        Picker::new(
            PathBuf::from("/tmp/root"),
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
}
