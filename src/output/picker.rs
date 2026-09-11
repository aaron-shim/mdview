//! 마크다운 파일 브라우저 TUI: Local 탭(현재 디렉터리)과 Stashed 탭(즐겨찾기).

use crate::source::Source;
use crate::stash::Stash;
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState, Paragraph};
use ratatui::{DefaultTerminal, Frame};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Local,
    Stashed,
}

enum Mode {
    Normal,
    Filter,
    /// 스태시 항목 메모 편집 (대상 인덱스, 입력 중인 텍스트)
    Note(usize, String),
}

pub struct Picker {
    root: PathBuf,
    files: Vec<PathBuf>,
    stash: Rc<RefCell<Stash>>,
    tab: Tab,
    filtered: Vec<usize>,
    state: ListState,
    filter: String,
    mode: Mode,
    status: Option<(String, Instant)>,
}

pub enum PickerExit {
    Quit,
    /// (소스, 표시 제목)
    Open(Source, String),
}

const STATUS_TTL: Duration = Duration::from_secs(2);

impl Picker {
    pub fn new(root: PathBuf, files: Vec<PathBuf>, stash: Rc<RefCell<Stash>>) -> Self {
        let mut p = Picker {
            root,
            files,
            stash,
            tab: Tab::Local,
            filtered: Vec::new(),
            state: ListState::default(),
            filter: String::new(),
            mode: Mode::Normal,
            status: None,
        };
        p.apply_filter();
        p
    }

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

    fn apply_filter(&mut self) {
        let q = self.filter.to_lowercase();
        self.filtered = (0..self.item_count()).filter(|&i| q.is_empty() || self.item_text(i).to_lowercase().contains(&q)).collect();
        if self.filtered.is_empty() {
            self.state.select(None);
        } else {
            let sel = self.state.selected().unwrap_or(0).min(self.filtered.len() - 1);
            self.state.select(Some(sel));
        }
    }

    fn switch_tab(&mut self, tab: Tab) {
        if self.tab != tab {
            self.tab = tab;
            self.filter.clear();
            self.state.select(Some(0));
            self.apply_filter();
        }
    }

    fn move_sel(&mut self, delta: isize) {
        if self.filtered.is_empty() {
            return;
        }
        let cur = self.state.selected().unwrap_or(0) as isize;
        let n = self.filtered.len() as isize;
        let next = (cur + delta).clamp(0, n - 1);
        self.state.select(Some(next as usize));
    }

    /// 현재 선택된 항목의 원본 인덱스
    fn selected_index(&self) -> Option<usize> {
        self.state.selected().and_then(|s| self.filtered.get(s).copied())
    }

    fn set_status(&mut self, msg: impl Into<String>) {
        self.status = Some((msg.into(), Instant::now()));
    }

    fn stash_selected(&mut self) {
        let Some(i) = self.selected_index() else { return };
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
        let Some(i) = self.selected_index() else { return };
        let result = {
            let mut st = self.stash.borrow_mut();
            st.remove(i);
            st.save()
        };
        match result {
            Ok(()) => self.set_status("Removed from stash"),
            Err(e) => self.set_status(format!("Save failed: {e:#}")),
        }
        self.apply_filter();
    }

    fn open_selected(&self) -> Option<PickerExit> {
        let i = self.selected_index()?;
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

    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<PickerExit> {
        // 페이저에서 돌아왔을 때 스태시가 바뀌었을 수 있다.
        self.apply_filter();
        loop {
            terminal.draw(|f| self.draw(f))?;
            if !event::poll(Duration::from_millis(250))? {
                continue;
            }
            let Event::Key(k) = event::read()? else { continue };
            if k.kind == KeyEventKind::Release {
                continue;
            }
            let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
            match &mut self.mode {
                Mode::Filter => {
                    match k.code {
                        KeyCode::Esc => {
                            self.mode = Mode::Normal;
                            self.filter.clear();
                            self.apply_filter();
                        }
                        KeyCode::Enter => self.mode = Mode::Normal,
                        KeyCode::Backspace => {
                            self.filter.pop();
                            self.apply_filter();
                        }
                        KeyCode::Char(c) if !ctrl => {
                            self.filter.push(c);
                            self.apply_filter();
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
            let page = terminal.size()?.height.saturating_sub(3) as isize;
            match k.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(PickerExit::Quit),
                KeyCode::Char('c') if ctrl => return Ok(PickerExit::Quit),
                KeyCode::Tab | KeyCode::BackTab => {
                    let next = if self.tab == Tab::Local { Tab::Stashed } else { Tab::Local };
                    self.switch_tab(next);
                }
                KeyCode::Char('1') => self.switch_tab(Tab::Local),
                KeyCode::Char('2') => self.switch_tab(Tab::Stashed),
                KeyCode::Char('j') | KeyCode::Down => self.move_sel(1),
                KeyCode::Char('k') | KeyCode::Up => self.move_sel(-1),
                KeyCode::PageDown => self.move_sel(page),
                KeyCode::PageUp => self.move_sel(-page),
                KeyCode::Char('d') if ctrl => self.move_sel(page),
                KeyCode::Char('u') if ctrl => self.move_sel(-page),
                KeyCode::Char('g') | KeyCode::Home => self.move_sel(isize::MIN / 2),
                KeyCode::Char('G') | KeyCode::End => self.move_sel(isize::MAX / 2),
                KeyCode::Char('/') => self.mode = Mode::Filter,
                KeyCode::Char('s') if self.tab == Tab::Local => self.stash_selected(),
                KeyCode::Char('x') if self.tab == Tab::Stashed => self.remove_selected(),
                KeyCode::Char('m') if self.tab == Tab::Stashed => {
                    if let Some(i) = self.selected_index() {
                        let cur = self.stash.borrow().entries[i].note.clone().unwrap_or_default();
                        self.mode = Mode::Note(i, cur);
                    }
                }
                KeyCode::Enter => {
                    if let Some(exit) = self.open_selected() {
                        return Ok(exit);
                    }
                }
                _ => {}
            }
        }
    }

    fn draw(&mut self, f: &mut Frame) {
        let [header, body, footer] = Layout::vertical([Constraint::Length(1), Constraint::Min(1), Constraint::Length(1)]).areas(f.area());

        // 헤더: 앱 이름 + 탭
        let tab_style = |active: bool| {
            if active {
                Style::default().fg(Color::Indexed(110)).add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
            } else {
                Style::default().fg(Color::Indexed(245))
            }
        };
        let n_stash = self.stash.borrow().entries.len();
        let title = Line::from(vec![
            Span::styled(" mdview ", Style::default().fg(Color::Indexed(231)).bg(Color::Indexed(24)).add_modifier(Modifier::BOLD)),
            Span::raw("  "),
            Span::styled("Local", tab_style(self.tab == Tab::Local)),
            Span::raw("   "),
            Span::styled(format!("Stashed ({n_stash})"), tab_style(self.tab == Tab::Stashed)),
            Span::styled(
                format!("   {}", if self.tab == Tab::Local { crate::hangul::compose(&crate::stash::shorten_home(&self.root.to_string_lossy())) } else { String::new() }),
                Style::default().fg(Color::Indexed(245)),
            ),
        ]);
        f.render_widget(Paragraph::new(title), header);

        // 목록
        let dim = Style::default().fg(Color::Indexed(245));
        let name_style = Style::default().fg(Color::Indexed(252));
        let items: Vec<ListItem> = match self.tab {
            Tab::Local => self
                .filtered
                .iter()
                .map(|&i| {
                    let p = &self.files[i];
                    // macOS의 나뉜 자소를 화면에 나올 때만 합친다.
                    let name = p.file_name().map(|n| crate::hangul::compose(&n.to_string_lossy())).unwrap_or_default();
                    let dir = p.parent().map(|d| crate::hangul::compose(&d.to_string_lossy())).unwrap_or_default();
                    let mut spans = vec![Span::raw("  "), Span::styled(name, name_style)];
                    if !dir.is_empty() && dir != "." {
                        spans.push(Span::styled(format!("  {dir}/"), dim));
                    }
                    ListItem::new(Line::from(spans))
                })
                .collect(),
            Tab::Stashed => {
                let st = self.stash.borrow();
                self.filtered
                    .iter()
                    .map(|&i| {
                        let e = &st.entries[i];
                        let mut spans = vec![Span::raw("  "), Span::styled(e.display_name(), name_style)];
                        let dir = e.display_dir();
                        if !dir.is_empty() {
                            spans.push(Span::styled(format!("  {dir}/"), dim));
                        }
                        if let Some(n) = &e.note {
                            spans.push(Span::styled(format!("  — {n}"), Style::default().fg(Color::Indexed(66))));
                        }
                        ListItem::new(Line::from(spans))
                    })
                    .collect()
            }
        };
        let list = List::new(items)
            .highlight_style(Style::default().fg(Color::Indexed(110)).add_modifier(Modifier::BOLD))
            .highlight_symbol("▌ ");
        f.render_stateful_widget(list, body, &mut self.state);

        // 푸터
        if self.status.as_ref().is_some_and(|(_, t)| t.elapsed() > STATUS_TTL) {
            self.status = None;
        }
        let footer_text = match &self.mode {
            Mode::Filter => format!("/{}", self.filter),
            Mode::Note(_, text) => format!("note: {text}▏  (Enter save, Esc cancel)"),
            Mode::Normal => {
                if let Some((msg, _)) = &self.status {
                    msg.clone()
                } else if self.filtered.is_empty() {
                    match self.tab {
                        Tab::Local => "no markdown files found  Tab stashed  q quit".to_string(),
                        Tab::Stashed => "stash is empty — press s on a file, or: mdview stash FILE|URL  Tab local  q quit".to_string(),
                    }
                } else {
                    match self.tab {
                        Tab::Local => format!("{} files  j/k move  Enter open  s stash  / filter  Tab stashed  q quit", self.filtered.len()),
                        Tab::Stashed => format!("{} stashed  j/k move  Enter open  x remove  m note  / filter  Tab local  q quit", self.filtered.len()),
                    }
                }
            }
        };
        f.render_widget(Paragraph::new(Span::styled(format!(" {footer_text}"), dim)), footer);
    }
}
