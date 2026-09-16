//! 입력 로드(파일, 표준입력, URL)와 마크다운 파일 탐색.

use crate::hangul;
use anyhow::{Context, Result, bail};
use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    Stdin,
    File(PathBuf),
    Url(String),
}

impl Source {
    /// 스태시에 저장할 식별자: 절대 경로 또는 URL. 표준입력은 저장할 수 없다.
    pub fn stash_key(&self) -> Option<String> {
        match self {
            Source::Stdin => None,
            Source::File(p) => Some(p.canonicalize().unwrap_or_else(|_| p.clone()).to_string_lossy().into_owned()),
            Source::Url(u) => Some(u.clone()),
        }
    }

    /// 스태시 항목 문자열(경로 또는 URL)에서 소스를 만든다.
    pub fn from_stash_key(key: &str) -> Source {
        if is_url(key) { Source::Url(key.to_string()) } else { Source::File(PathBuf::from(key)) }
    }
}

pub struct Document {
    pub title: String,
    pub text: String,
    pub source: Source,
}

pub fn load(source: &Source) -> Result<Document> {
    match source {
        Source::Stdin => read_stdin(),
        Source::File(p) => read_file(p),
        Source::Url(u) => fetch_url(u),
    }
}

pub fn read_stdin() -> Result<Document> {
    let mut text = String::new();
    std::io::stdin().read_to_string(&mut text).context("failed to read stdin")?;
    Ok(Document { title: "stdin".into(), text: hangul::compose(&text), source: Source::Stdin })
}

pub fn read_file(path: &Path) -> Result<Document> {
    if path.is_dir() {
        bail!("{} is a directory", path.display());
    }
    let bytes = std::fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
    // macOS 등에서 자소가 나뉜 채 저장된 한글을 음절로 합친다.
    let text = hangul::compose(&String::from_utf8_lossy(&bytes));
    let title = hangul::compose(&path.display().to_string());
    Ok(Document { title, text, source: Source::File(path.to_path_buf()) })
}

/// 명령줄 인자가 URL처럼 보이는지. `github.com/...` 같은 스킴 없는 형태도 허용한다.
pub fn looks_like_url(arg: &str) -> bool {
    if is_url(arg) {
        return true;
    }
    const HOSTS: &[&str] = &["github.com/", "raw.githubusercontent.com/", "gist.github.com/", "gitlab.com/"];
    HOSTS.iter().any(|h| arg.starts_with(h))
}

pub fn is_url(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://")
}

/// 입력 URL을 실제로 내려받을 URL로 바꾼다.
/// - 스킴이 없으면 https:// 를 붙인다.
/// - github.com/{owner}/{repo} → 저장소 README
/// - github.com/{owner}/{repo}/blob/{ref}/{path} → raw 파일
pub fn normalize_url(input: &str) -> String {
    let with_scheme = if is_url(input) { input.to_string() } else { format!("https://{input}") };
    let rest = with_scheme.trim_start_matches("https://").trim_start_matches("http://");
    let Some(path) = rest.strip_prefix("github.com/") else {
        return with_scheme;
    };
    let path = path.trim_end_matches('/');
    let parts: Vec<&str> = path.split('/').collect();
    match parts.as_slice() {
        [owner, repo] => format!("https://raw.githubusercontent.com/{owner}/{repo}/HEAD/README.md"),
        [owner, repo, "blob" | "raw", r#ref, file @ ..] if !file.is_empty() => {
            format!("https://raw.githubusercontent.com/{owner}/{repo}/{ref}/{}", file.join("/"))
        }
        _ => with_scheme,
    }
}

pub fn fetch_url(url: &str) -> Result<Document> {
    let target = normalize_url(url);
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .user_agent(concat!("mdview/", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    let mut resp = match agent.get(&target).call() {
        Ok(r) => r,
        Err(ureq::Error::StatusCode(code)) => bail!("{target}: HTTP {code}"),
        Err(ureq::Error::Timeout(_)) => bail!("{target}: request timed out"),
        Err(e) => bail!("{target}: {e}"),
    };
    let text = resp.body_mut().read_to_string().with_context(|| format!("cannot read body of {target}"))?;
    Ok(Document { title: url.to_string(), text: hangul::compose(&text), source: Source::Url(url.to_string()) })
}

pub fn stdin_is_tty() -> bool {
    std::io::stdin().is_terminal()
}

pub fn stdout_is_tty() -> bool {
    std::io::stdout().is_terminal()
}

const MD_EXTS: &[&str] = &["md", "markdown", "mdown", "mkd", "mkdn"];

pub fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| MD_EXTS.iter().any(|m| m.eq_ignore_ascii_case(e)))
        .unwrap_or(false)
}

/// 파일이 바뀌었는지 알아채기 위한 표식: (수정 시각, 크기).
pub type Stamp = (Option<SystemTime>, u64);

/// 한 파일의 현재 표식. 읽을 수 없으면 None.
pub fn stamp_of(path: &Path) -> Option<Stamp> {
    let m = std::fs::metadata(path).ok()?;
    Some((m.modified().ok(), m.len()))
}

/// `root` 아래의 마크다운 파일을 .gitignore를 존중하며 찾는다. 결과는 root 기준 상대 경로이며,
/// 변경 감지에 쓸 표식을 함께 돌려준다.
///
/// `hidden`이 참이면 `.`으로 시작하는 파일·폴더까지 훑는다. 저장소 내부(`.git`)는 어느 쪽이든 들어가지 않는다.
///
/// 다른 파일 시스템(`/proc`, 네트워크 마운트 등)으로는 내려가지 않는다.
/// 상위 폴더로 올라가며 넓은 범위를 훑을 때 엉뚱한 곳까지 뒤지지 않게 하려는 것이다.
pub fn scan_markdown_files(root: &Path, hidden: bool) -> Vec<(PathBuf, Stamp)> {
    scan_markdown_files_with(root, hidden, |_| true).unwrap_or_default()
}

/// 경로에 `.`으로 시작하는 조각이 있는지: 숨김 파일이거나 숨김 폴더 안에 있는지.
pub fn is_hidden_path(path: &Path) -> bool {
    path.iter().any(|c| c.to_string_lossy().starts_with('.'))
}

/// 중간 보고를 넣는 간격: 파일이 이만큼 더 모였을 때, 또는 `PROGRESS_INTERVAL`이 지났을 때.
pub(crate) const PROGRESS_EVERY: usize = 64;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(150);
/// 시계를 보는 빈도(항목 수). 매 항목마다 보지 않아도 충분히 촘촘하다.
const CLOCK_EVERY: usize = 64;

/// `scan_markdown_files`와 같되, 훑는 도중 `tick`에 지금까지 모은(정렬된) 목록을 넘긴다.
/// `tick`이 `false`를 돌려주면 그 자리에서 멈추고 `None`.
/// 넓은 폴더에서 화면을 먼저 채우거나 훑기를 취소할 때 쓴다.
pub fn scan_markdown_files_with(root: &Path, hidden: bool, mut tick: impl FnMut(&[(PathBuf, Stamp)]) -> bool) -> Option<Vec<(PathBuf, Stamp)>> {
    let mut files: Vec<(PathBuf, Stamp)> = Vec::new();
    let mut reported = 0;
    let mut visited = 0usize;
    let mut last_tick = Instant::now();
    // `.git` 안은 문서가 아니라 저장소 데이터다. 숨김을 켜도 들어가지 않는다.
    let walk = ignore::WalkBuilder::new(root).hidden(!hidden).max_depth(Some(6)).same_file_system(true).filter_entry(|e| e.depth() == 0 || e.file_name() != ".git").build();
    for e in walk.filter_map(|e| e.ok()) {
        if e.file_type().is_some_and(|t| t.is_file()) && is_markdown(e.path()) {
            let stamp = e.metadata().map(|m| (m.modified().ok(), m.len())).unwrap_or((None, 0));
            let rel = e.path().strip_prefix(root).map(Path::to_path_buf).unwrap_or_else(|_| e.path().to_path_buf());
            files.push((rel, stamp));
        }
        visited += 1;
        let due = files.len() - reported >= PROGRESS_EVERY || (visited.is_multiple_of(CLOCK_EVERY) && last_tick.elapsed() >= PROGRESS_INTERVAL);
        if due {
            let mut snap = files.clone();
            sort_files(&mut snap);
            if !tick(&snap) {
                return None;
            }
            reported = files.len();
            last_tick = Instant::now();
        }
    }
    sort_files(&mut files);
    Some(files)
}

/// 얕은 것부터, 같은 깊이면 경로순.
pub(crate) fn sort_files(files: &mut [(PathBuf, Stamp)]) {
    files.sort_by(|(a, _), (b, _)| {
        let da = a.components().count();
        let db = b.components().count();
        da.cmp(&db).then_with(|| a.cmp(b))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_markdown_extensions() {
        assert!(is_markdown(Path::new("a.md")));
        assert!(is_markdown(Path::new("a.MD")));
        assert!(is_markdown(Path::new("a.markdown")));
        assert!(!is_markdown(Path::new("a.txt")));
        assert!(!is_markdown(Path::new("README")));
    }

    #[test]
    fn finds_files_recursively() {
        let dir = std::env::temp_dir().join(format!("mdview-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("b.md"), "x").unwrap();
        std::fs::write(dir.join("a.txt"), "x").unwrap();
        std::fs::write(dir.join("sub/c.markdown"), "x").unwrap();
        let files: Vec<PathBuf> = scan_markdown_files(&dir, false).into_iter().map(|(p, _)| p).collect();
        assert_eq!(files, vec![PathBuf::from("b.md"), PathBuf::from("sub/c.markdown")]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn hidden_files_and_folders_are_found_only_when_asked() {
        let dir = std::env::temp_dir().join(format!("mdview-hidden-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".github")).unwrap();
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::write(dir.join("a.md"), "x").unwrap();
        std::fs::write(dir.join(".secret.md"), "x").unwrap();
        std::fs::write(dir.join(".github/PULL_REQUEST_TEMPLATE.md"), "x").unwrap();
        std::fs::write(dir.join(".git/inside.md"), "x").unwrap();

        let plain: Vec<PathBuf> = scan_markdown_files(&dir, false).into_iter().map(|(p, _)| p).collect();
        assert_eq!(plain, vec![PathBuf::from("a.md")]);

        let all: Vec<PathBuf> = scan_markdown_files(&dir, true).into_iter().map(|(p, _)| p).collect();
        assert_eq!(all, vec![PathBuf::from(".secret.md"), PathBuf::from("a.md"), PathBuf::from(".github/PULL_REQUEST_TEMPLATE.md")]);
        assert!(!all.iter().any(|p| p.starts_with(".git/")), ".git 안은 숨김을 켜도 들어가지 않는다");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn hidden_paths_are_recognised_at_any_depth() {
        assert!(is_hidden_path(Path::new(".a.md")));
        assert!(is_hidden_path(Path::new(".github/a.md")));
        assert!(is_hidden_path(Path::new("docs/.draft/a.md")));
        assert!(!is_hidden_path(Path::new("docs/a.md")));
    }

    #[test]
    fn url_detection() {
        assert!(looks_like_url("https://a.b/c.md"));
        assert!(looks_like_url("http://a.b"));
        assert!(looks_like_url("github.com/o/r"));
        assert!(!looks_like_url("README.md"));
        assert!(!looks_like_url("docs/github.com"));
    }

    #[test]
    fn github_urls_are_rewritten_to_raw() {
        assert_eq!(normalize_url("github.com/charmbracelet/glow"), "https://raw.githubusercontent.com/charmbracelet/glow/HEAD/README.md");
        assert_eq!(normalize_url("https://github.com/o/r/"), "https://raw.githubusercontent.com/o/r/HEAD/README.md");
        assert_eq!(normalize_url("https://github.com/o/r/blob/main/docs/a.md"), "https://raw.githubusercontent.com/o/r/main/docs/a.md");
        assert_eq!(normalize_url("https://github.com/o/r/issues/1"), "https://github.com/o/r/issues/1");
        assert_eq!(normalize_url("https://example.com/x.md"), "https://example.com/x.md");
        assert_eq!(normalize_url("example.com/x.md"), "https://example.com/x.md");
    }

    #[test]
    fn stash_key_roundtrip() {
        assert_eq!(Source::Stdin.stash_key(), None);
        let u = Source::Url("https://a.b/c.md".into());
        assert_eq!(u.stash_key().as_deref(), Some("https://a.b/c.md"));
        assert_eq!(Source::from_stash_key("https://a.b/c.md"), u);
        assert_eq!(Source::from_stash_key("/tmp/a.md"), Source::File(PathBuf::from("/tmp/a.md")));
    }

    fn many_files(name: &str, n: usize) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mdview-scan-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        for i in 0..n {
            std::fs::write(dir.join(if i % 2 == 0 { "" } else { "sub" }).join(format!("f{i:04}.md")), "x").unwrap();
        }
        dir
    }

    #[test]
    fn scan_reports_progress_before_it_finishes() {
        let dir = many_files("progress", PROGRESS_EVERY * 3 + 5);
        let mut seen = Vec::new();
        let all = scan_markdown_files_with(&dir, false, |partial| {
            seen.push(partial.len());
            true
        })
        .expect("취소하지 않았으니 결과가 있다");
        assert_eq!(all.len(), PROGRESS_EVERY * 3 + 5);
        assert!(seen.len() >= 3, "중간 보고가 여러 번 온다: {seen:?}");
        assert!(seen.windows(2).all(|w| w[0] <= w[1]), "보고할 때마다 목록이 줄지 않는다: {seen:?}");
        assert!(seen.iter().any(|&n| n < all.len()), "완료 전에 보고한다: {seen:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_stops_when_the_callback_says_so() {
        let dir = many_files("cancel", PROGRESS_EVERY * 3);
        let mut calls = 0;
        let out = scan_markdown_files_with(&dir, false, |_| {
            calls += 1;
            false
        });
        assert!(out.is_none(), "취소하면 결과를 돌려주지 않는다");
        assert_eq!(calls, 1, "첫 보고에서 바로 멈춘다");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn progress_lists_are_sorted_like_the_final_one() {
        let dir = many_files("sorted", PROGRESS_EVERY + 1);
        let mut first = None;
        scan_markdown_files_with(&dir, false, |partial| {
            first.get_or_insert_with(|| partial.to_vec());
            true
        });
        let first = first.unwrap();
        let mut sorted = first.clone();
        sort_files(&mut sorted);
        assert_eq!(first, sorted);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
