//! 이미지를 반블록(`▀`) 문자와 트루컬러로 그린다.
//!
//! 한 칸에 위아래 두 픽셀을 담는다: 글자색이 윗 픽셀, 배경색이 아랫 픽셀.
//! 그래픽 프로토콜(kitty, sixel)이 없는 터미널에서도 보이고, 문서 모델의 줄로
//! 나오므로 페이저·미리보기·그대로 출력 어디서나 같은 방식으로 쓴다.

use crate::doc::{Color, Line, Span, Style};
use image::RgbaImage;
use image::imageops::FilterType;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

/// 상대 경로 이미지를 찾을 기준.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImageBase {
    /// 문서가 있는 폴더
    Dir(PathBuf),
    /// 원격 문서의 (내려받은) URL
    Url(String),
}

/// 읽어 둔 이미지의 최대 크기(픽셀). 터미널 폭보다 넉넉하면 충분하다.
const MAX_W: u32 = 480;
const MAX_H: u32 = 960;
/// 원격 이미지 최대 크기(바이트)
const MAX_BYTES: u64 = 16 * 1024 * 1024;
/// 캐시가 이만큼 넘으면 비운다.
const CACHE_LIMIT: usize = 64;

type Cache = Mutex<HashMap<String, Option<Arc<RgbaImage>>>>;

fn cache() -> &'static Cache {
    static CACHE: OnceLock<Cache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 이미지 주소가 실제로 가리키는 곳.
#[derive(Debug, PartialEq, Eq)]
enum Loc {
    File(PathBuf),
    Url(String),
}

fn resolve(src: &str, base: &ImageBase) -> Option<Loc> {
    let src = src.trim();
    if src.is_empty() || src.starts_with("data:") || src.starts_with('#') {
        return None;
    }
    if crate::source::is_url(src) {
        // github.com/.../blob/... 은 HTML 페이지이므로 raw 주소로 바꾼다.
        let url = if src.contains("github.com/") && (src.contains("/blob/") || src.contains("/raw/")) {
            crate::source::normalize_url(src)
        } else {
            src.to_string()
        };
        return Some(Loc::Url(url));
    }
    if let Some(p) = src.strip_prefix("file://") {
        return Some(Loc::File(PathBuf::from(p)));
    }
    // 쿼리·조각은 파일 이름이 아니다.
    let path = src.split(['?', '#']).next().unwrap_or(src);
    match base {
        ImageBase::Dir(dir) => {
            let p = Path::new(path);
            Some(Loc::File(if p.is_absolute() { p.to_path_buf() } else { dir.join(p) }))
        }
        ImageBase::Url(u) => Some(Loc::Url(join_url(u, src))),
    }
}

/// 기준 URL에 상대 주소를 잇는다.
fn join_url(base: &str, rel: &str) -> String {
    if let Some(rest) = rel.strip_prefix("//") {
        let scheme = base.split("://").next().unwrap_or("https");
        return format!("{scheme}://{rest}");
    }
    let (scheme, after) = base.split_once("://").unwrap_or(("https", base));
    let host_end = after.find('/').unwrap_or(after.len());
    let host = &after[..host_end];
    if let Some(abs) = rel.strip_prefix('/') {
        return format!("{scheme}://{host}/{abs}");
    }
    // 기준의 마지막 조각(파일 이름)을 떼고 `.`/`..`를 정리한다.
    let path = after[host_end..].split(['?', '#']).next().unwrap_or("");
    let mut parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if !path.ends_with('/') {
        parts.pop();
    }
    for seg in rel.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    format!("{scheme}://{host}/{}", parts.join("/"))
}

fn cache_key(loc: &Loc) -> String {
    match loc {
        // 파일이 바뀌면 다시 읽도록 수정 시각·크기를 키에 넣는다.
        Loc::File(p) => format!("{}\0{:?}", p.display(), crate::source::stamp_of(p)),
        Loc::Url(u) => u.clone(),
    }
}

fn load(loc: &Loc) -> Option<Arc<RgbaImage>> {
    let key = cache_key(loc);
    if let Some(hit) = cache().lock().ok()?.get(&key) {
        return hit.clone();
    }
    let img = read_bytes(loc).and_then(|b| decode(&b)).map(Arc::new);
    if let Ok(mut c) = cache().lock() {
        if c.len() >= CACHE_LIMIT {
            c.clear();
        }
        c.insert(key, img.clone());
    }
    img
}

fn read_bytes(loc: &Loc) -> Option<Vec<u8>> {
    match loc {
        Loc::File(p) => {
            if std::fs::metadata(p).ok()?.len() > MAX_BYTES {
                return None;
            }
            std::fs::read(p).ok()
        }
        Loc::Url(u) => {
            // SVG는 그릴 수 없으니 내려받지 않는다(배지 등).
            if u.split(['?', '#']).next().is_some_and(|p| p.to_ascii_lowercase().ends_with(".svg")) {
                return None;
            }
            let agent: ureq::Agent = ureq::Agent::config_builder()
                .timeout_global(Some(Duration::from_secs(5)))
                .user_agent(concat!("mdview/", env!("CARGO_PKG_VERSION")))
                .build()
                .into();
            let mut resp = agent.get(u).call().ok()?;
            resp.body_mut().with_config().limit(MAX_BYTES).read_to_vec().ok()
        }
    }
}

fn decode(bytes: &[u8]) -> Option<RgbaImage> {
    let img = image::load_from_memory(bytes).ok()?;
    let img = if img.width() > MAX_W || img.height() > MAX_H { img.thumbnail(MAX_W, MAX_H) } else { img };
    Some(img.to_rgba8())
}

/// 이미지를 읽어 `width` 칸, 최대 `max_rows` 줄 안에 맞춰 그린다. 읽지 못하면 None.
pub fn render(src: &str, base: &ImageBase, width: usize, max_rows: usize) -> Option<Vec<Line>> {
    let loc = resolve(src, base)?;
    let img = load(&loc)?;
    Some(to_lines(&img, width, max_rows))
}

/// 표시할 칸 수와 줄 수. 한 칸이 가로 1 × 세로 2 픽셀이라 줄 수는 픽셀 높이의 절반이다.
fn fit(w: u32, h: u32, width: usize, max_rows: usize) -> (u32, u32) {
    let (w, h) = (w.max(1) as f64, h.max(1) as f64);
    let mut cols = (width as f64).min(w).max(1.0);
    let mut rows = (cols * h / w / 2.0).ceil().max(1.0);
    if rows > max_rows as f64 {
        rows = max_rows.max(1) as f64;
        cols = (rows * 2.0 * w / h).round().clamp(1.0, width.max(1) as f64);
    }
    (cols as u32, rows as u32)
}

fn to_lines(img: &RgbaImage, width: usize, max_rows: usize) -> Vec<Line> {
    let (cols, rows) = fit(img.width(), img.height(), width, max_rows);
    let small = image::imageops::resize(img, cols, rows * 2, FilterType::Triangle);
    let px = |x: u32, y: u32| {
        let p = small.get_pixel(x, y).0;
        // 반투명은 반쯤 넘으면 보이는 것으로 친다.
        (p[3] >= 128).then_some(Color::Rgb(p[0], p[1], p[2]))
    };
    (0..rows)
        .map(|r| {
            let mut line = Line::new();
            for x in 0..cols {
                let cell = match (px(x, r * 2), px(x, r * 2 + 1)) {
                    (Some(t), Some(b)) => Span::new("▀", Style::new().fg(t).bg(b)),
                    (Some(t), None) => Span::new("▀", Style::new().fg(t)),
                    (None, Some(b)) => Span::new("▄", Style::new().fg(b)),
                    (None, None) => Span::raw(" "),
                };
                // 같은 모양이 이어지면 한 조각으로 묶는다.
                match line.spans.last_mut() {
                    Some(last) if last.style == cell.style && last.text.ends_with(cell.text.as_str()) => last.text.push_str(&cell.text),
                    _ => line.push(cell),
                }
            }
            line
        })
        .collect()
}

/// HTML 조각에서 `<img src="...">`의 주소를 모두 꺼낸다.
pub fn html_img_srcs(html: &str) -> Vec<String> {
    let lower = html.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = lower[from..].find("<img") {
        let start = from + i;
        let end = lower[start..].find('>').map_or(lower.len(), |e| start + e);
        if let Some(src) = attr(&html[start..end], &lower[start..end], "src") {
            out.push(src);
        }
        from = end;
    }
    out
}

fn attr(tag: &str, lower: &str, name: &str) -> Option<String> {
    let mut from = 0;
    while let Some(i) = lower[from..].find(name) {
        let at = from + i;
        from = at + name.len();
        // `data-src` 같은 다른 속성의 일부가 아닌지 본다.
        if !lower[..at].ends_with(|c: char| c.is_ascii_whitespace()) {
            continue;
        }
        let rest = lower[from..].trim_start();
        let Some(rest) = rest.strip_prefix('=') else { continue };
        let val_start = lower.len() - rest.trim_start().len();
        let raw = &tag[val_start..];
        let val = match raw.chars().next() {
            Some(q @ ('"' | '\'')) => raw[1..].split(q).next().unwrap_or(""),
            _ => raw.split(|c: char| c.is_ascii_whitespace() || c == '/').next().unwrap_or(""),
        };
        return Some(val.to_string());
    }
    None
}

/// 태그를 걷어내면 글자가 남지 않는지 (이미지만 담은 HTML인지).
pub fn html_has_no_text(html: &str) -> bool {
    let mut depth = 0;
    for c in html.chars() {
        match c {
            '<' => depth += 1,
            '>' => depth = (depth - 1).max(0),
            c if depth == 0 && !c.is_whitespace() => return false,
            _ => {}
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_relative_urls() {
        let base = "https://raw.githubusercontent.com/o/r/HEAD/docs/README.md";
        assert_eq!(join_url(base, "img/a.png"), "https://raw.githubusercontent.com/o/r/HEAD/docs/img/a.png");
        assert_eq!(join_url(base, "./a.png"), "https://raw.githubusercontent.com/o/r/HEAD/docs/a.png");
        assert_eq!(join_url(base, "../a.png"), "https://raw.githubusercontent.com/o/r/HEAD/a.png");
        assert_eq!(join_url(base, "/x.png"), "https://raw.githubusercontent.com/x.png");
        assert_eq!(join_url(base, "//cdn.io/x.png"), "https://cdn.io/x.png");
    }

    #[test]
    fn resolves_against_dir_and_github_blob() {
        let base = ImageBase::Dir(PathBuf::from("/d"));
        assert_eq!(resolve("a/b.png?raw=1", &base), Some(Loc::File(PathBuf::from("/d/a/b.png"))));
        assert_eq!(resolve("/abs.png", &base), Some(Loc::File(PathBuf::from("/abs.png"))));
        assert_eq!(
            resolve("https://github.com/o/r/blob/main/a.png", &base),
            Some(Loc::Url("https://raw.githubusercontent.com/o/r/main/a.png".into()))
        );
        assert_eq!(resolve("data:image/png;base64,xx", &base), None);
    }

    #[test]
    fn fits_width_and_height() {
        // 가로 200 × 세로 100 → 폭 80 칸, 20 줄
        assert_eq!(fit(200, 100, 80, 40), (80, 20));
        // 작은 이미지는 키우지 않는다
        assert_eq!(fit(10, 10, 80, 40), (10, 5));
        // 세로로 긴 이미지는 줄 수에 맞춰 줄인다
        assert_eq!(fit(100, 1000, 80, 20), (4, 20));
    }

    #[test]
    fn half_blocks_carry_two_pixels() {
        let mut img = RgbaImage::new(2, 2);
        img.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));
        img.put_pixel(0, 1, image::Rgba([0, 0, 255, 255]));
        img.put_pixel(1, 0, image::Rgba([0, 0, 0, 0]));
        img.put_pixel(1, 1, image::Rgba([0, 255, 0, 255]));
        let lines = to_lines(&img, 80, 40);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].plain(), "▀▄");
        assert_eq!(lines[0].spans[0].style, Style::new().fg(Color::Rgb(255, 0, 0)).bg(Color::Rgb(0, 0, 255)));
        assert_eq!(lines[0].spans[1].style, Style::new().fg(Color::Rgb(0, 255, 0)));
    }

    #[test]
    fn finds_img_src_in_html() {
        let html = r#"<p align="center"><img data-src="no" src="logo.png" width=200><IMG SRC=b.jpg /></p>"#;
        assert_eq!(html_img_srcs(html), vec!["logo.png", "b.jpg"]);
        assert!(html_has_no_text(html));
        assert!(!html_has_no_text("<p>hi <img src=a.png></p>"));
    }
}
