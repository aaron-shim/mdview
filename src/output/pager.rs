//! ratatui 기반 페이저: 스크롤, 검색, 종료.

use crate::doc::Line;
use crate::output::tui_convert;
use crate::stash::Stash;
use std::cell::RefCell;
use std::rc::Rc;
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line as RLine, Span};
use ratatui::widgets::Paragraph;
use ratatui::{DefaultTerminal, Frame};
use std::time::{Duration, Instant};

pub enum PagerExit {
    Quit,
    /// 파일 브라우저로 돌아가기
    Back,
}

pub struct Pager {
    title: String,
    lines: Vec<Line>,
    rendered: Vec<RLine<'static>>,
    scroll: usize,
    search: Option<String>,
    search_input: Option<String>,
    matches: Vec<usize>,
    match_idx: usize,
    /// 파일 브라우저에서 열렸는지 (Esc/Backspace로 돌아갈 수 있음)
    from_picker: bool,
    /// 폭 변경 시 다시 렌더링하기 위한 콜백
    rerender: Box<dyn Fn(usize) -> Vec<Line>>,
    width: usize,
    stash: Rc<RefCell<Stash>>,
    /// 스태시에 저장할 식별자. 표준입력 문서는 None.
    stash_key: Option<String>,
    status: Option<(String, Instant)>,
}

const STATUS_TTL: Duration = Duration::from_secs(2);

impl Pager {
    pub fn new(
        title: String,
        width: usize,
        rerender: Box<dyn Fn(usize) -> Vec<Line>>,
        from_picker: bool,
        stash: Rc<RefCell<Stash>>,
        stash_key: Option<String>,
    ) -> Self {
        let lines = rerender(width);
        let rendered = lines.iter().map(tui_convert::line).collect();
        Pager {
            title,
            lines,
            rendered,
            scroll: 0,
            search: None,
            search_input: None,
            matches: Vec::new(),
            match_idx: 0,
            from_picker,
            rerender,
            width,
            stash,
            stash_key,
            status: None,
        }
    }

    fn set_status(&mut self, msg: impl Into<String>) {
        self.status = Some((msg.into(), Instant::now()));
    }

    fn stash_current(&mut self) {
        let Some(key) = self.stash_key.clone() else {
            self.set_status("Cannot stash stdin");
            return;
        };
        let result = {
            let mut st = self.stash.borrow_mut();
            let added = st.add(&key, None);
            st.save().map(|_| added)
        };
        match result {
            Ok(true) => self.set_status("Stashed ✓"),
            Ok(false) => self.set_status("Already stashed"),
            Err(e) => self.set_status(format!("Stash failed: {e:#}")),
        }
    }

    fn set_width(&mut self, w: usize) {
        if w != self.width && w > 0 {
            self.width = w;
            self.lines = (self.rerender)(w);
            self.rendered = self.lines.iter().map(tui_convert::line).collect();
            self.scroll = self.scroll.min(self.lines.len().saturating_sub(1));
            self.recompute_matches();
        }
    }

    fn recompute_matches(&mut self) {
        self.matches.clear();
        if let Some(q) = &self.search {
            if q.is_empty() {
                return;
            }
            let q = q.to_lowercase();
            for (i, l) in self.lines.iter().enumerate() {
                if l.plain().to_lowercase().contains(&q) {
                    self.matches.push(i);
                }
            }
        }
    }

    fn jump_to_match(&mut self, forward: bool, page: usize) {
        if self.matches.is_empty() {
            return;
        }
        if forward {
            // 현재 스크롤 아래의 첫 매치
            let next = self.matches.iter().position(|&m| m > self.scroll).unwrap_or(0);
            self.match_idx = next;
        } else {
            let prev = self.matches.iter().rposition(|&m| m < self.scroll).unwrap_or(self.matches.len() - 1);
            self.match_idx = prev;
        }
        self.scroll = self.matches[self.match_idx].min(self.max_scroll(page));
    }

    fn max_scroll(&self, page: usize) -> usize {
        self.lines.len().saturating_sub(page)
    }

    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<PagerExit> {
        loop {
            terminal.draw(|f| self.draw(f))?;
            if !event::poll(Duration::from_millis(250))? {
                continue;
            }
            let ev = event::read()?;
            let size = terminal.size()?;
            let page = size.height.saturating_sub(2) as usize;
            match ev {
                Event::Key(k) if k.kind != KeyEventKind::Release => {
                    if let Some(input) = self.search_input.as_mut() {
                        match k.code {
                            KeyCode::Esc => self.search_input = None,
                            KeyCode::Enter => {
                                let q = self.search_input.take().unwrap_or_default();
                                self.search = if q.is_empty() { None } else { Some(q) };
                                self.recompute_matches();
                                self.jump_to_match(true, page);
                            }
                            KeyCode::Backspace => {
                                input.pop();
                            }
                            KeyCode::Char(c) => input.push(c),
                            _ => {}
                        }
                        continue;
                    }
                    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
                    match k.code {
                        KeyCode::Char('q') => return Ok(PagerExit::Quit),
                        KeyCode::Char('c') if ctrl => return Ok(PagerExit::Quit),
                        KeyCode::Esc | KeyCode::Backspace if self.from_picker => return Ok(PagerExit::Back),
                        KeyCode::Esc => {
                            self.search = None;
                            self.matches.clear();
                        }
                        KeyCode::Char('j') | KeyCode::Down => self.scroll = (self.scroll + 1).min(self.max_scroll(page)),
                        KeyCode::Char('k') | KeyCode::Up => self.scroll = self.scroll.saturating_sub(1),
                        KeyCode::Char('d') if ctrl => self.scroll = (self.scroll + page / 2).min(self.max_scroll(page)),
                        KeyCode::Char('u') if ctrl => self.scroll = self.scroll.saturating_sub(page / 2),
                        KeyCode::Char('f') if ctrl => self.scroll = (self.scroll + page).min(self.max_scroll(page)),
                        KeyCode::Char('b') if ctrl => self.scroll = self.scroll.saturating_sub(page),
                        KeyCode::PageDown | KeyCode::Char(' ') => self.scroll = (self.scroll + page).min(self.max_scroll(page)),
                        KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(page),
                        KeyCode::Char('g') | KeyCode::Home => self.scroll = 0,
                        KeyCode::Char('G') | KeyCode::End => self.scroll = self.max_scroll(page),
                        KeyCode::Char('/') => self.search_input = Some(String::new()),
                        KeyCode::Char('s') => self.stash_current(),
                        KeyCode::Char('n') => self.jump_to_match(true, page),
                        KeyCode::Char('N') => self.jump_to_match(false, page),
                        _ => {}
                    }
                }
                Event::Mouse(m) => match m.kind {
                    event::MouseEventKind::ScrollDown => self.scroll = (self.scroll + 3).min(self.max_scroll(page)),
                    event::MouseEventKind::ScrollUp => self.scroll = self.scroll.saturating_sub(3),
                    _ => {}
                },
                Event::Resize(_, _) => {}
                _ => {}
            }
        }
    }

    fn draw(&mut self, f: &mut Frame) {
        let area = f.area();
        let [header, body, footer] = Layout::vertical([Constraint::Length(1), Constraint::Min(1), Constraint::Length(1)]).areas(area);
        self.set_width(area.width as usize);

        let page = body.height as usize;
        self.scroll = self.scroll.min(self.max_scroll(page));

        // 헤더
        let title = RLine::from(vec![
            Span::styled(" mdview ", Style::default().fg(Color::Indexed(231)).bg(Color::Indexed(24)).add_modifier(Modifier::BOLD)),
            Span::styled(format!(" {}", self.title), Style::default().fg(Color::Indexed(245))),
        ]);
        f.render_widget(Paragraph::new(title), header);

        // 본문
        let end = (self.scroll + page).min(self.rendered.len());
        let mut visible: Vec<RLine<'static>> = self.rendered[self.scroll..end].to_vec();
        if let Some(q) = &self.search {
            let ql = q.to_lowercase();
            for (i, line) in visible.iter_mut().enumerate() {
                let abs = self.scroll + i;
                if self.matches.contains(&abs) {
                    *line = highlight_line(&self.lines[abs], &ql, abs == self.matches.get(self.match_idx).copied().unwrap_or(usize::MAX));
                }
            }
        }
        f.render_widget(Paragraph::new(visible), body);

        // 푸터
        let pct = if self.lines.len() <= page { 100 } else { (end * 100) / self.lines.len().max(1) };
        if self.status.as_ref().is_some_and(|(_, t)| t.elapsed() > STATUS_TTL) {
            self.status = None;
        }
        let footer_text = if let Some(inp) = &self.search_input {
            format!("/{inp}")
        } else if let Some((msg, _)) = &self.status {
            msg.clone()
        } else if let Some(q) = &self.search {
            format!("/{q}  {} matches  n/N next/prev  Esc clear", self.matches.len())
        } else if self.from_picker {
            "j/k scroll  / search  s stash  Esc back  q quit".to_string()
        } else {
            "j/k scroll  / search  s stash  q quit".to_string()
        };
        let footer_line = RLine::from(vec![
            Span::styled(format!(" {pct:>3}% "), Style::default().fg(Color::Indexed(231)).bg(Color::Indexed(240))),
            Span::styled(format!(" {footer_text}"), Style::default().fg(Color::Indexed(245))),
        ]);
        f.render_widget(Paragraph::new(footer_line), footer);
    }
}

/// 검색어와 일치하는 부분을 강조한 줄을 만든다.
fn highlight_line(line: &Line, query_lower: &str, current: bool) -> RLine<'static> {
    let mut spans = Vec::new();
    let hl = if current {
        Style::default().fg(Color::Indexed(231)).bg(Color::Indexed(25))
    } else {
        Style::default().fg(Color::Indexed(232)).bg(Color::Indexed(110))
    };
    for s in &line.spans {
        let base = tui_convert::style(s.style);
        let lower = s.text.to_lowercase();
        // 소문자화로 바이트 길이가 달라질 수 있으므로 길이가 다르면 강조를 생략한다.
        if lower.len() != s.text.len() || query_lower.is_empty() {
            spans.push(Span::styled(s.text.clone(), base));
            continue;
        }
        let mut pos = 0;
        while let Some(idx) = lower[pos..].find(query_lower) {
            let start = pos + idx;
            let end = start + query_lower.len();
            if start > pos {
                spans.push(Span::styled(s.text[pos..start].to_string(), base));
            }
            spans.push(Span::styled(s.text[start..end].to_string(), hl));
            pos = end;
        }
        if pos < s.text.len() {
            spans.push(Span::styled(s.text[pos..].to_string(), base));
        }
    }
    RLine::from(spans)
}
