//! 마크다운 → 스타일 줄 목록 렌더러.

pub mod canvas;
pub mod code;
pub mod html_table;
pub mod image;
pub mod math;
pub mod mermaid;
pub mod table;

use crate::doc::{Line, Span, Style};
use crate::theme::{Theme, ThemeKind};
use crate::wrap::wrap_line;
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use image::{Images, Picture};
use std::sync::Arc;
use table::Table;

/// 블록 접두(들여쓰기, 인용 막대, 목록 기호).
#[derive(Clone, Debug)]
struct Prefix {
    /// 블록의 첫 줄에 쓰는 접두 (목록 기호 등)
    first: Vec<Span>,
    /// 이후 줄에 쓰는 접두
    rest: Vec<Span>,
    used: bool,
    is_item: bool,
}

impl Prefix {
    fn same(spans: Vec<Span>) -> Self {
        Prefix { first: spans.clone(), rest: spans, used: false, is_item: false }
    }
    fn width(&self) -> usize {
        self.rest.iter().map(Span::width).sum()
    }
}

struct ListState {
    next_index: Option<u64>,
    loose: bool,
    items: usize,
}

struct Renderer<'t> {
    theme: &'t Theme,
    width: usize,
    out: Vec<Line>,
    prefixes: Vec<Prefix>,
    /// 인라인 스타일 스택
    styles: Vec<Style>,
    /// 현재 조립 중인 인라인 줄
    inline: Line,
    /// 강제 줄바꿈으로 나뉜 문단 조각들
    para: Vec<Line>,
    lists: Vec<ListState>,
    /// 다음 블록 앞에 빈 줄이 필요한지
    need_blank: bool,
    code: Option<(String, String)>, // (lang, buffer)
    table: Option<Table>,
    row: Vec<table::Cell>,
    in_table_head: bool,
    link_url: Option<String>,
    image_url: Option<String>,
    /// `<table>` 판별을 위해 모아 두는 HTML 블록
    html_block: Option<String>,
    /// 목록 항목 첫 줄에 붙을 체크박스
    skip_depth: usize,
    footnotes: Vec<(String, Vec<Line>)>,
    /// 각주 본문을 잡아두는 동안 잠시 치워둔 (출력, 접두)
    saved: Option<(Vec<Line>, Vec<Prefix>)>,
    /// 이미지 설정. None이면 이미지를 그리지 않고 대체 글만 보인다.
    images: Option<Images>,
    /// 출력기가 픽셀로 그릴 이미지 자리
    placements: Vec<Placement>,
    /// 이미지를 그릴 최대 줄 수
    image_rows: usize,
    /// 지금 문단에 나온 이미지 주소. 문단이 이미지 하나뿐이면 그림으로 그린다.
    para_image: Option<String>,
    /// 지금 문단에 이미지 말고 다른 내용이 있는지
    para_extra: bool,
}

/// 출력기가 픽셀(sixel)로 그릴 이미지 한 장의 자리.
#[derive(Clone)]
pub struct Placement {
    /// 이미지가 시작하는 줄
    pub line: usize,
    /// 이미지가 시작하는 칸(왼쪽 접두 너비)
    pub col: usize,
    /// 차지하는 줄 수
    pub rows: usize,
    /// 한 줄의 픽셀 높이
    pub cell_h: u32,
    pub image: Arc<crate::sixel::Indexed>,
}

/// 렌더링 결과: 줄들과, 그 위에 픽셀로 그릴 이미지 자리.
pub struct Rendered {
    pub lines: Vec<Line>,
    pub images: Vec<Placement>,
}

/// HTML 이미지 블록의 조각.
enum HtmlPart {
    Picture(Picture),
    Line(Line),
}

/// 이미지 한 장이 차지할 수 있는 최대 줄 수: 터미널 높이에서 조금 뺀 만큼.
fn image_rows() -> usize {
    crossterm::terminal::size().ok().map(|(_, h)| h as usize).filter(|h| *h > 0).map_or(40, |h| h.saturating_sub(4).max(8))
}

/// 평문 한 줄을 폭에 맞춰 나눈다.
pub fn wrap_plain(s: &str, width: usize) -> Vec<String> {
    wrap_line(&Line::from_spans(vec![Span::raw(s)]), width).iter().map(Line::plain).collect()
}

/// 마크다운 문자열을 `width` 폭에 맞춘 스타일 줄들로 렌더링한다. 이미지는 대체 글로만 보인다.
#[cfg(test)]
pub fn render(markdown: &str, theme: &Theme, width: usize) -> Vec<Line> {
    render_doc(markdown, theme, width, None).lines
}

/// `render`와 같되, `images`가 있으면 홀로 선 이미지를 그림으로 그린다.
/// 색이 없는 테마에서는 그리지 않는다.
pub fn render_doc(markdown: &str, theme: &Theme, width: usize, images: Option<&Images>) -> Rendered {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_FOOTNOTES);
    opts.insert(Options::ENABLE_YAML_STYLE_METADATA_BLOCKS);
    opts.insert(Options::ENABLE_HEADING_ATTRIBUTES);
    opts.insert(Options::ENABLE_MATH);
    let parser = Parser::new_ext(markdown, opts);

    let mut r = Renderer {
        theme,
        width: width.max(10),
        out: Vec::new(),
        prefixes: vec![Prefix::same(vec![Span::raw(" ".repeat(theme.margin))])],
        styles: vec![theme.text],
        inline: Line::new(),
        para: Vec::new(),
        lists: Vec::new(),
        need_blank: false,
        code: None,
        table: None,
        row: Vec::new(),
        in_table_head: false,
        link_url: None,
        image_url: None,
        html_block: None,
        skip_depth: 0,
        footnotes: Vec::new(),
        saved: None,
        images: images.filter(|_| theme.kind != ThemeKind::NoTty).cloned(),
        placements: Vec::new(),
        image_rows: image_rows(),
        para_image: None,
        para_extra: false,
    };
    for ev in parser {
        r.event(ev);
    }
    r.finish();
    Rendered { lines: r.out, images: r.placements }
}

impl<'t> Renderer<'t> {
    fn style(&self) -> Style {
        *self.styles.last().unwrap()
    }
    fn push_style(&mut self, s: Style) {
        let merged = self.style().merge(s);
        self.styles.push(merged);
    }
    fn pop_style(&mut self) {
        if self.styles.len() > 1 {
            self.styles.pop();
        }
    }

    fn prefix_width(&self) -> usize {
        self.prefixes.iter().map(Prefix::width).sum()
    }
    fn avail(&self) -> usize {
        self.width.saturating_sub(self.prefix_width()).max(4)
    }

    /// 현재 접두를 조합해 반환하고, 첫 줄 접두는 사용된 것으로 표시한다.
    fn take_prefix(&mut self) -> Vec<Span> {
        let mut spans = Vec::new();
        for p in &mut self.prefixes {
            if p.used {
                spans.extend(p.rest.iter().cloned());
            } else {
                spans.extend(p.first.iter().cloned());
                p.used = true;
            }
        }
        spans
    }

    /// 접두를 붙여 출력에 한 줄 추가한다.
    fn emit(&mut self, line: Line) {
        let mut spans = self.take_prefix();
        spans.extend(line.spans);
        let mut l = Line::from_spans(spans);
        trim_end(&mut l);
        self.out.push(l);
    }

    /// 블록 사이의 빈 줄. 인용 안에서는 막대만 남긴다.
    fn blank_if_needed(&mut self) {
        if self.need_blank && !self.out.is_empty() {
            let spans: Vec<Span> = self.prefixes.iter().flat_map(|p| p.rest.iter().cloned()).collect();
            let mut l = Line::from_spans(spans);
            trim_end(&mut l);
            self.out.push(l);
        }
        self.need_blank = false;
    }

    fn text(&mut self, s: &str) {
        let style = self.style();
        self.inline.push(Span::new(s, style));
    }

    fn hard_break(&mut self) {
        let l = std::mem::take(&mut self.inline);
        self.para.push(l);
    }

    /// 지금까지의 인라인 내용을 줄바꿈해 출력한다.
    fn flush_inline(&mut self) {
        let mut segs = std::mem::take(&mut self.para);
        let cur = std::mem::take(&mut self.inline);
        if !cur.is_empty() || segs.is_empty() {
            segs.push(cur);
        }
        if segs.iter().all(Line::is_empty) {
            return;
        }
        self.blank_if_needed();
        let avail = self.avail();
        for seg in segs {
            for l in wrap_line(&seg, avail) {
                self.emit(l);
            }
        }
    }

    fn finish(&mut self) {
        self.flush_inline();
        if !self.footnotes.is_empty() {
            self.need_blank = true;
            self.blank_if_needed();
            let rule = Line::from_spans(vec![Span::new("─".repeat(self.avail().min(20)), self.theme.rule)]);
            self.emit(rule);
            let notes = std::mem::take(&mut self.footnotes);
            for (label, lines) in notes {
                let marker = Span::new(format!("[{label}] "), self.theme.link_url);
                let w = marker.width();
                self.prefixes.push(Prefix { first: vec![marker], rest: vec![Span::raw(" ".repeat(w))], used: false, is_item: false });
                for l in lines {
                    self.emit(l);
                }
                self.prefixes.pop();
            }
        }
        // 마지막 빈 줄 제거. 이미지 자리는 비어 보여도 남긴다.
        let keep = self.placements.iter().map(|p| p.line + p.rows).max().unwrap_or(0);
        while self.out.len() > keep && self.out.last().is_some_and(|l| l.is_empty() || l.plain().trim().is_empty()) {
            self.out.pop();
        }
    }

    /// 이미지를 읽어 그린다. 그릴 수 없으면 None.
    fn picture(&self, src: &str) -> Option<Picture> {
        let images = self.images.as_ref()?;
        image::render(src, images, self.avail(), self.image_rows)
    }

    /// 그림을 출력에 넣는다. 픽셀 그림은 빈 줄로 자리를 잡고 위치를 적어 둔다.
    fn emit_picture(&mut self, pic: Picture) {
        match pic {
            Picture::Cells(lines) => {
                for l in lines {
                    self.emit(l);
                }
            }
            // 각주 본문은 나중에 옮겨 붙으므로 줄 위치를 알 수 없다. 대체 글만 남긴다.
            Picture::Pixels { .. } if self.saved.is_some() => {}
            Picture::Pixels { rows, cell_h, image } => {
                let placement = Placement { line: self.out.len(), col: self.prefix_width(), rows, cell_h, image };
                for _ in 0..rows {
                    self.emit(Line::new());
                }
                self.placements.push(placement);
            }
        }
    }

    /// 이미지만 담은 HTML 블록(`<p align="center"><img src=...></p>` 등)의 그림들.
    /// 글이 섞였거나 하나도 그리지 못하면 None.
    fn html_pictures(&self, html: &str) -> Option<Vec<HtmlPart>> {
        self.images.as_ref()?;
        let srcs = image::html_img_srcs(html);
        if srcs.is_empty() || !image::html_has_no_text(html) {
            return None;
        }
        let mut out = Vec::new();
        let mut drawn = false;
        for src in &srcs {
            match self.picture(src) {
                Some(pic) => {
                    if !out.is_empty() {
                        out.push(HtmlPart::Line(Line::new()));
                    }
                    out.push(HtmlPart::Picture(pic));
                    drawn = true;
                }
                None => {
                    let st = self.theme.image;
                    let mut l = Line::from_spans(vec![Span::new("🖼 ", st)]);
                    l.push(Span::new(format!("({src})"), st.merge(self.theme.link_url)));
                    out.push(HtmlPart::Line(l));
                }
            }
        }
        drawn.then_some(out)
    }

    fn heading_level(level: HeadingLevel) -> usize {
        match level {
            HeadingLevel::H1 => 1,
            HeadingLevel::H2 => 2,
            HeadingLevel::H3 => 3,
            HeadingLevel::H4 => 4,
            HeadingLevel::H5 => 5,
            HeadingLevel::H6 => 6,
        }
    }

    fn event(&mut self, ev: Event<'_>) {
        if self.skip_depth > 0 {
            match ev {
                Event::Start(_) => self.skip_depth += 1,
                Event::End(_) => self.skip_depth -= 1,
                _ => {}
            }
            return;
        }
        match ev {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(t) => {
                if let Some((_, buf)) = self.code.as_mut() {
                    buf.push_str(&t);
                } else {
                    if self.image_url.is_none() && !t.trim().is_empty() {
                        self.para_extra = true;
                    }
                    self.text(&t);
                }
            }
            Event::Code(t) => {
                self.para_extra = true;
                let st = self.style().merge(self.theme.code);
                self.inline.push(Span::new(format!(" {t} "), st));
            }
            Event::InlineMath(t) => {
                self.para_extra = true;
                let st = self.style().merge(self.theme.math);
                self.inline.push(Span::new(math::render_inline(&t), st));
            }
            Event::DisplayMath(t) => {
                self.flush_inline();
                self.blank_if_needed();
                let avail = self.avail();
                for l in math::render_block(&t, self.theme, avail) {
                    self.emit(l);
                }
                self.need_blank = true;
            }
            Event::Html(h) => {
                if let Some(buf) = self.html_block.as_mut() {
                    buf.push_str(&h);
                    return;
                }
                self.flush_inline();
                let st = self.style().merge(self.theme.html);
                for l in h.lines() {
                    self.inline.push(Span::new(l, st));
                    self.hard_break();
                }
                self.flush_inline();
                self.need_blank = true;
            }
            Event::InlineHtml(h) => {
                // `<br>`는 줄바꿈으로 바꾼다(표 칸 안에서도 쓰인다).
                let tag = h.trim().to_ascii_lowercase();
                if tag == "<br>" || tag == "<br/>" || tag == "<br />" {
                    self.hard_break();
                    return;
                }
                self.para_extra = true;
                let st = self.style().merge(self.theme.html);
                self.inline.push(Span::new(h.to_string(), st));
            }
            Event::SoftBreak => self.text(" "),
            Event::HardBreak => self.hard_break(),
            Event::Rule => {
                self.flush_inline();
                self.blank_if_needed();
                let w = self.avail();
                self.emit(Line::from_spans(vec![Span::new("─".repeat(w), self.theme.rule)]));
                self.need_blank = true;
            }
            Event::FootnoteReference(name) => {
                self.para_extra = true;
                let st = self.style().merge(self.theme.link_url);
                self.inline.push(Span::new(format!("[{name}]"), st));
            }
            Event::TaskListMarker(done) => {
                // 목록 항목 접두의 기호를 체크박스로 바꾼다.
                let (mark, st) = if done { ("✓ ", self.theme.task_done) } else { ("☐ ", self.theme.task_todo) };
                if let Some(p) = self.prefixes.last_mut()
                    && !p.used {
                        let mut first = p.first.clone();
                        first.push(Span::new(mark, st));
                        let w: usize = first.iter().map(Span::width).sum();
                        p.first = first;
                        p.rest = vec![Span::raw(" ".repeat(w))];
                    }
            }
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => {
                self.flush_inline();
                self.para_image = None;
                self.para_extra = false;
                if self.prefixes.last().is_some_and(|p| p.is_item)
                    && let Some(l) = self.lists.last_mut() {
                        l.loose = true;
                    }
            }
            Tag::Heading { level, .. } => {
                self.flush_inline();
                let lv = Self::heading_level(level);
                self.need_blank = true;
                self.blank_if_needed();
                self.push_style(self.theme.heading[lv - 1]);
            }
            Tag::BlockQuote(_) => {
                self.flush_inline();
                self.blank_if_needed();
                let bar = Span::new("│ ", self.theme.quote_bar);
                self.prefixes.push(Prefix::same(vec![bar]));
                self.push_style(self.theme.quote);
            }
            Tag::CodeBlock(kind) => {
                self.flush_inline();
                let lang = match kind {
                    CodeBlockKind::Fenced(l) => l.to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                self.code = Some((lang, String::new()));
            }
            Tag::List(start) => {
                self.flush_inline();
                if self.lists.is_empty() {
                    self.blank_if_needed();
                } else {
                    self.need_blank = false;
                }
                self.lists.push(ListState { next_index: start, loose: false, items: 0 });
            }
            Tag::Item => {
                self.flush_inline();
                let (loose, items) = self.lists.last().map(|l| (l.loose, l.items)).unwrap_or((false, 0));
                self.need_blank = loose && items > 0;
                self.blank_if_needed();
                if let Some(l) = self.lists.last_mut() {
                    l.items += 1;
                }
                let marker = match self.lists.last_mut() {
                    Some(ListState { next_index: Some(n), .. }) => {
                        let m = format!("{n}. ");
                        *n += 1;
                        m
                    }
                    _ => {
                        let depth = self.lists.len();
                        let bullet = match depth % 3 {
                            1 => "•",
                            2 => "◦",
                            _ => "▪",
                        };
                        format!("{bullet} ")
                    }
                };
                let w = marker.chars().count();
                let marker = Span::new(marker, self.theme.list_marker);
                self.prefixes.push(Prefix { first: vec![marker], rest: vec![Span::raw(" ".repeat(w))], used: false, is_item: true });
            }
            Tag::Table(aligns) => {
                self.flush_inline();
                self.blank_if_needed();
                self.table = Some(Table::new(aligns));
            }
            Tag::TableHead => {
                self.in_table_head = true;
                self.row.clear();
            }
            Tag::TableRow => {
                self.row.clear();
            }
            Tag::TableCell => {
                self.inline = Line::new();
                self.para.clear();
            }
            Tag::Emphasis => self.push_style(self.theme.emphasis),
            Tag::Strong => self.push_style(self.theme.strong),
            Tag::Strikethrough => self.push_style(self.theme.strikethrough),
            Tag::Link { dest_url, .. } => {
                self.link_url = Some(dest_url.to_string());
                self.push_style(self.theme.link);
            }
            Tag::Image { dest_url, .. } => {
                if self.para_image.is_some() {
                    self.para_extra = true;
                }
                self.para_image = Some(dest_url.to_string());
                self.image_url = Some(dest_url.to_string());
                self.push_style(self.theme.image);
                let st = self.style();
                self.inline.push(Span::new("🖼 ", st));
            }
            Tag::FootnoteDefinition(name) => {
                self.flush_inline();
                self.footnotes.push((name.to_string(), Vec::new()));
                let out = std::mem::take(&mut self.out);
                let prefixes = std::mem::take(&mut self.prefixes);
                self.saved = Some((out, prefixes));
                self.width = self.width.saturating_sub(6);
                self.need_blank = false;
            }
            Tag::HtmlBlock => {
                self.flush_inline();
                self.html_block = Some(String::new());
            }
            Tag::MetadataBlock(_) => {
                // 프론트매터는 출력하지 않는다.
                self.skip_depth = 1;
            }
            Tag::DefinitionListTitle => {
                self.flush_inline();
                self.push_style(self.theme.strong);
            }
            Tag::DefinitionListDefinition => {
                self.flush_inline();
                self.prefixes.push(Prefix::same(vec![Span::raw("    ")]));
            }
            Tag::DefinitionList | Tag::Superscript | Tag::Subscript => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => {
                if let Some(src) = self.para_image.take()
                    && !self.para_extra
                    && let Some(pic) = self.picture(&src)
                {
                    // 대체 글과 주소는 그림 아래 설명으로 남긴다.
                    self.blank_if_needed();
                    self.emit_picture(pic);
                }
                self.flush_inline();
                self.need_blank = true;
            }
            TagEnd::Heading(level) => {
                let lv = Self::heading_level(level);
                let start = self.out.len();
                self.flush_inline();
                self.pop_style();
                // 제목 아래 줄은 제목 글자 너비만큼만 긋는다.
                if let Some(ch) = self.theme.heading_rule[lv - 1] {
                    let prefix = self.prefix_width();
                    let text_w = self.out[start..].iter().map(Line::width).max().unwrap_or(0).saturating_sub(prefix);
                    let w = text_w.clamp(1, self.avail());
                    // 줄 자체는 굵게 하지 않아 제목보다 튀지 않게 둔다.
                    let mut st = if lv == 1 { self.theme.heading[0] } else { self.theme.rule };
                    st.bold = false;
                    self.emit(Line::from_spans(vec![Span::new(ch.to_string().repeat(w), st)]));
                }
                self.need_blank = true;
            }
            TagEnd::BlockQuote(_) => {
                self.flush_inline();
                self.prefixes.pop();
                self.pop_style();
                self.need_blank = true;
            }
            TagEnd::CodeBlock => {
                let Some((lang, buf)) = self.code.take() else { return };
                self.blank_if_needed();
                // mermaid 코드블록은 그림으로 그린다. 못 그리면 원문을 보여준다.
                if lang.trim().eq_ignore_ascii_case("mermaid")
                    && let Some(diagram) = mermaid::render(&buf, self.theme, self.avail().saturating_sub(2))
                {
                    self.prefixes.push(Prefix::same(vec![Span::raw("  ")]));
                    for l in diagram {
                        self.emit(l);
                    }
                    self.prefixes.pop();
                    self.need_blank = true;
                    return;
                }
                let lines = code::highlight(&buf, &lang, self.theme);
                self.prefixes.push(Prefix::same(vec![Span::raw("  ")]));
                for l in lines {
                    self.emit(l);
                }
                self.prefixes.pop();
                self.need_blank = true;
            }
            TagEnd::List(_) => {
                self.flush_inline();
                self.lists.pop();
                self.need_blank = true;
            }
            TagEnd::Item => {
                self.flush_inline();
                // 내용이 전혀 없는 항목도 기호는 출력한다.
                if let Some(p) = self.prefixes.last()
                    && !p.used {
                        self.emit(Line::new());
                    }
                self.prefixes.pop();
                self.need_blank = false;
            }
            TagEnd::Table => {
                if let Some(t) = self.table.take() {
                    let lines = t.render(self.theme, self.avail());
                    for l in lines {
                        self.emit(l);
                    }
                }
                self.need_blank = true;
            }
            TagEnd::TableHead => {
                let row = std::mem::take(&mut self.row);
                if let Some(t) = self.table.as_mut() {
                    t.header = row;
                }
                self.in_table_head = false;
            }
            TagEnd::TableRow => {
                let row = std::mem::take(&mut self.row);
                if let Some(t) = self.table.as_mut() {
                    t.rows.push(row);
                }
            }
            TagEnd::TableCell => {
                // `<br>`로 나뉜 조각까지 모아 한 칸으로 만든다.
                let mut cell = std::mem::take(&mut self.para);
                cell.push(std::mem::take(&mut self.inline));
                self.row.push(cell);
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => self.pop_style(),
            TagEnd::Link => {
                self.pop_style();
                if let Some(url) = self.link_url.take() {
                    let text = self.inline.plain();
                    if !text.trim_end().ends_with(&url) && !url.is_empty() {
                        let st = self.style().merge(self.theme.link_url);
                        self.inline.push(Span::new(format!(" ({url})"), st));
                    }
                }
            }
            TagEnd::Image => {
                self.pop_style();
                if let Some(url) = self.image_url.take() {
                    let st = self.style().merge(self.theme.link_url);
                    self.inline.push(Span::new(format!(" ({url})"), st));
                }
            }
            TagEnd::FootnoteDefinition => {
                self.flush_inline();
                let body = std::mem::take(&mut self.out);
                if let Some((out, prefixes)) = self.saved.take() {
                    self.out = out;
                    self.prefixes = prefixes;
                }
                self.width += 6;
                if let Some(last) = self.footnotes.last_mut() {
                    last.1.extend(body);
                }
                self.need_blank = false;
            }
            TagEnd::HtmlBlock => {
                if let Some(buf) = self.html_block.take() {
                    self.blank_if_needed();
                    // `<table>` 이면 표로, 이미지만 있으면 그림으로 그리고, 아니면 원문을 그대로 보여준다.
                    if let Some(lines) = html_table::render(&buf, self.theme, self.avail()) {
                        for l in lines {
                            self.emit(l);
                        }
                    } else if let Some(parts) = self.html_pictures(&buf) {
                        for part in parts {
                            match part {
                                HtmlPart::Picture(p) => self.emit_picture(p),
                                HtmlPart::Line(l) => self.emit(l),
                            }
                        }
                    } else {
                        {
                            // 표가 아니면 원문을 폭에 맞춰 그대로 보여준다.
                            let st = self.theme.html;
                            let avail = self.avail();
                            for l in buf.lines() {
                                if l.trim().is_empty() {
                                    continue;
                                }
                                for w in wrap_line(&Line::from_spans(vec![Span::new(l.trim_end(), st)]), avail) {
                                    self.emit(w);
                                }
                            }
                        }
                    }
                }
                self.need_blank = true;
            }
            TagEnd::MetadataBlock(_) => {}
            TagEnd::DefinitionListTitle => {
                self.flush_inline();
                self.pop_style();
            }
            TagEnd::DefinitionListDefinition => {
                self.flush_inline();
                self.prefixes.pop();
            }
            TagEnd::DefinitionList => {
                self.need_blank = true;
            }
            TagEnd::Superscript | TagEnd::Subscript => {}
        }
    }
}

fn trim_end(line: &mut Line) {
    while let Some(last) = line.spans.last_mut() {
        let n = last.text.trim_end_matches(' ').len();
        last.text.truncate(n);
        if last.text.is_empty() {
            line.spans.pop();
        } else {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(md: &str, width: usize) -> Vec<String> {
        let theme = Theme { margin: 0, ..Theme::notty() };
        render(md, &theme, width).iter().map(Line::plain).collect()
    }

    #[test]
    fn paragraph_wraps() {
        assert_eq!(plain("hello world foo bar", 11), vec!["hello world", "foo bar"]);
    }

    #[test]
    fn paragraphs_separated_by_blank() {
        assert_eq!(plain("a\n\nb", 80), vec!["a", "", "b"]);
    }

    #[test]
    fn headings_are_underlined_not_prefixed() {
        assert_eq!(
            plain("# Title\n\ntext\n\n## Sub\n\n### Small", 80),
            vec!["Title", "━━━━━", "", "text", "", "Sub", "───", "", "Small"]
        );
    }

    #[test]
    fn heading_rule_matches_wrapped_width() {
        // 줄바꿈된 제목은 가장 긴 줄에 맞춘다.
        assert_eq!(plain("# one two three", 9), vec!["one two", "three", "━━━━━━━"]);
    }

    #[test]
    fn heading_keeps_style_without_hashes() {
        let theme = Theme::dark();
        let out = render("## 제목", &theme, 40);
        assert_eq!(out[0].plain().trim(), "제목");
        assert!(out[0].spans.iter().any(|s| s.text.contains("제목") && s.style.bold));
    }

    #[test]
    fn unordered_and_ordered_lists() {
        assert_eq!(plain("- a\n- b\n\n1. x\n2. y", 80), vec!["• a", "• b", "", "1. x", "2. y"]);
    }

    #[test]
    fn nested_list_indents() {
        assert_eq!(plain("- a\n  - b\n  - c\n- d", 80), vec!["• a", "  ◦ b", "  ◦ c", "• d"]);
    }

    #[test]
    fn list_item_wraps_with_hanging_indent() {
        assert_eq!(plain("- one two three", 9), vec!["• one two", "  three"]);
    }

    #[test]
    fn task_list() {
        assert_eq!(plain("- [x] done\n- [ ] todo", 80), vec!["• ✓ done", "• ☐ todo"]);
    }

    #[test]
    fn blockquote_prefix() {
        assert_eq!(plain("> quoted text\n> more", 80), vec!["│ quoted text more"]);
    }

    #[test]
    fn code_block_indented() {
        assert_eq!(plain("```rust\nfn main() {}\n```", 80), vec!["  fn main() {}"]);
    }

    #[test]
    fn inline_code_padded() {
        assert_eq!(plain("use `x` now", 80), vec!["use  x  now"]);
    }

    #[test]
    fn link_shows_url() {
        assert_eq!(plain("[site](https://a.b)", 80), vec!["site (https://a.b)"]);
        assert_eq!(plain("<https://a.b>", 80), vec!["https://a.b"]);
    }

    #[test]
    fn image_shows_alt_and_url() {
        assert_eq!(plain("![alt](x.png)", 80), vec!["🖼 alt (x.png)"]);
    }

    #[test]
    fn standalone_image_is_drawn() {
        use crate::render::image::{ImageBase, ImageMode};
        let dir = std::env::temp_dir().join(format!("mdview-img-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        ::image::RgbaImage::from_pixel(8, 4, ::image::Rgba([255, 0, 0, 255])).save(dir.join("a.png")).unwrap();
        let cells = Images { base: ImageBase::Dir(dir.clone()), mode: ImageMode::Cells };
        let plain = |md: &str, theme: &Theme, images: &Images| -> Vec<String> {
            render_doc(md, theme, 80, Some(images)).lines.iter().map(|l| l.plain().trim_start().to_string()).collect()
        };
        assert_eq!(plain("![alt](a.png)", &Theme::dark(), &cells), vec!["▀▀▀▀▀▀▀▀", "▀▀▀▀▀▀▀▀", "🖼 alt (a.png)"], "그림 아래에 대체 글이 남는다");
        // 글 속 이미지와 없는 파일, 색 없는 테마는 대체 글만 보인다.
        assert_eq!(plain("see ![alt](a.png)", &Theme::dark(), &cells).len(), 1);
        assert_eq!(plain("![alt](none.png)", &Theme::dark(), &cells).len(), 1);
        assert_eq!(plain("![alt](a.png)", &Theme::notty(), &cells).len(), 1);
        assert_eq!(plain("<p align=\"center\"><img src=\"a.png\"></p>", &Theme::dark(), &cells), vec!["▀▀▀▀▀▀▀▀", "▀▀▀▀▀▀▀▀"]);

        // 픽셀 모드는 빈 줄로 자리를 잡고 위치를 알려 준다. 칸이 4×2 픽셀이면 8×4 그림은
        // sixel 6픽셀 단위로 올린 높이(6)를 담도록 3줄.
        let pixels = Images { base: ImageBase::Dir(dir.clone()), mode: ImageMode::Pixels { cell_w: 4, cell_h: 2 } };
        let r = render_doc("# 제목\n\n![alt](a.png)", &Theme::dark(), 80, Some(&pixels));
        assert_eq!(r.images.len(), 1);
        let p = &r.images[0];
        assert_eq!((p.line, p.rows, p.col, p.image.width, p.image.height), (3, 3, 2, 8, 4));
        assert!(r.lines[3..6].iter().all(Line::is_empty));
        assert_eq!(r.lines[6].plain().trim(), "🖼 alt (a.png)");
        // 문서 끝의 HTML 그림은 비어 보여도 자리가 잘리지 않는다.
        let r = render_doc("<img src=\"a.png\">", &Theme::dark(), 80, Some(&pixels));
        assert_eq!((r.lines.len(), r.images.len()), (3, 1));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rule_spans_width() {
        assert_eq!(plain("a\n\n---\n\nb", 10), vec!["a", "", "──────────", "", "b"]);
    }

    #[test]
    fn table_renders() {
        let out = plain("| a | b |\n|---|---|\n| 1 | 2 |", 80);
        assert_eq!(out[0], "┌───┬───┐");
        assert_eq!(out[1], "│ a │ b │");
        assert_eq!(out[3], "│ 1 │ 2 │");
    }

    #[test]
    fn hard_break_starts_new_line() {
        assert_eq!(plain("a  \nb", 80), vec!["a", "b"]);
    }

    #[test]
    fn frontmatter_skipped() {
        assert_eq!(plain("---\ntitle: x\n---\n\nbody", 80), vec!["body"]);
    }

    #[test]
    fn margin_applied() {
        let theme = Theme { margin: 2, ..Theme::notty() };
        let out: Vec<String> = render("hi", &theme, 80).iter().map(Line::plain).collect();
        assert_eq!(out, vec!["  hi"]);
    }

    #[test]
    fn styles_applied_in_dark_theme() {
        let theme = Theme::dark();
        let out = render("**bold**", &theme, 80);
        let bold = out[0].spans.iter().find(|s| s.text == "bold").unwrap();
        assert!(bold.style.bold);
    }

    #[test]
    fn quote_inside_list_and_blank_lines() {
        let out = plain("- item\n\n  > q\n\n- next", 80);
        assert_eq!(out, vec!["• item", "", "  │ q", "", "• next"]);
    }

    #[test]
    fn footnotes_rendered_at_end() {
        let out = plain("text[^1]\n\n[^1]: note", 80);
        assert_eq!(out[0], "text[1]");
        assert_eq!(out.last().unwrap(), "[1] note");
    }
}

#[cfg(test)]
mod integration {
    use super::*;

    /// 코드블록을 뺀 예제 문서는 어떤 폭에서도 폭을 넘지 않아야 한다.
    /// (코드블록은 원문 그대로 보여주는 것이 맞으므로 줄바꿈하지 않는다.)
    #[test]
    fn sample_document_fits_every_width() {
        let md = include_str!("../../examples/markdown-sample.md");
        let mut prose = String::new();
        let mut in_code = false;
        for line in md.lines() {
            if line.trim_start().starts_with("```") {
                in_code = !in_code;
                continue;
            }
            if !in_code && !line.starts_with("    ") {
                prose.push_str(line);
                prose.push('\n');
            }
        }
        for width in [40usize, 60, 80, 100, 120] {
            let lines = render(&prose, &Theme::notty(), width);
            assert!(!lines.is_empty());
            for (i, l) in lines.iter().enumerate() {
                assert!(l.width() <= width, "폭 {width}에서 {i}번째 줄이 {}칸: {}", l.width(), l.plain());
            }
        }
    }

    /// 다이어그램·수식이 든 문서 전체를 렌더링해도 패닉이 없어야 한다.
    #[test]
    fn sample_document_renders_at_any_width() {
        let md = include_str!("../../examples/markdown-sample.md");
        for width in [10usize, 25, 40, 77, 100, 160, 400] {
            let lines = render(md, &Theme::dark(), width);
            assert!(!lines.is_empty());
        }
    }

    #[test]
    fn mermaid_block_becomes_a_diagram() {
        let out: Vec<String> = render("```mermaid\nflowchart TB\n A --> B\n```", &Theme::notty(), 60).iter().map(Line::plain).collect();
        assert!(out[0].contains("mermaid · flowchart"));
        assert!(out.iter().any(|l| l.contains('▼')));
    }

    #[test]
    fn unsupported_mermaid_falls_back_to_code() {
        let out: Vec<String> = render("```mermaid\nmindmap\n  root((중심))\n```", &Theme::notty(), 60).iter().map(Line::plain).collect();
        assert!(out.iter().any(|l| l.contains("mindmap")));
        assert!(!out.iter().any(|l| l.contains("mermaid ·")));
    }

    #[test]
    fn display_math_is_centered_block() {
        let out: Vec<String> = render("$$\n\\frac{a}{b}\n$$", &Theme::notty(), 20).iter().map(Line::plain).collect();
        assert!(out.iter().any(|l| l.contains('─')));
        assert!(out.iter().any(|l| l.trim() == "a"));
    }

    #[test]
    fn inline_math_stays_in_the_paragraph() {
        let out: Vec<String> = render("값은 $x_1 + x_2$ 이다.", &Theme::notty(), 40).iter().map(Line::plain).collect();
        assert_eq!(out[0].trim(), "값은 x₁ + x₂ 이다.");
    }

    #[test]
    fn html_table_with_spans_is_drawn() {
        let md = "<table><tr><th colspan=\"2\">묶음</th></tr><tr><td>1</td><td>2</td></tr></table>";
        let out: Vec<String> = render(md, &Theme::notty(), 60).iter().map(Line::plain).collect();
        assert!(out.iter().any(|l| l.contains("묶음")));
        assert!(out.iter().any(|l| l.contains('┬')));
    }
}
