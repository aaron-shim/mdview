//! 로컬 즐겨찾기(스태시): 문서 경로나 URL을 메모와 함께 JSON 파일에 저장한다.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Entry {
    /// 절대 경로 또는 URL
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// 저장 시각 (unix 초)
    #[serde(default)]
    pub added: u64,
}

impl Entry {
    pub fn is_url(&self) -> bool {
        self.source.starts_with("http://") || self.source.starts_with("https://")
    }
    /// 목록에 보여줄 이름: 파일이면 파일명, URL이면 URL 전체.
    /// 목록에 보여 줄 이름. 저장된 키(`source`)는 그대로 두고 표시용으로만 한글 자소를 합친다.
    pub fn display_name(&self) -> String {
        if self.is_url() {
            return self.source.clone();
        }
        let name = Path::new(&self.source)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.source.clone());
        crate::hangul::compose(&name)
    }
    /// 파일이면 디렉터리, URL이면 빈 문자열.
    pub fn display_dir(&self) -> String {
        if self.is_url() {
            String::new()
        } else {
            let p = Path::new(&self.source);
            let dir = p.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
            crate::hangul::compose(&shorten_home(&dir))
        }
    }
}

/// `$HOME` 접두를 `~`로 줄인다.
pub fn shorten_home(path: &str) -> String {
    if let Some(home) = std::env::var_os("HOME") {
        let home = home.to_string_lossy();
        if let Some(rest) = path.strip_prefix(home.as_ref()) {
            return format!("~{rest}");
        }
    }
    path.to_string()
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct FileFormat {
    #[serde(default)]
    entries: Vec<Entry>,
}

#[derive(Debug)]
pub struct Stash {
    path: PathBuf,
    pub entries: Vec<Entry>,
}

/// 기본 저장 위치: `$MDVIEW_STASH` > `$XDG_CONFIG_HOME/mdview/stash.json` > `~/.config/mdview/stash.json`
pub fn default_path() -> PathBuf {
    if let Some(p) = std::env::var_os("MDVIEW_STASH") {
        return PathBuf::from(p);
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("mdview").join("stash.json")
}

impl Stash {
    pub fn load() -> Self {
        Self::load_from(default_path())
    }

    /// 파일이 없거나 깨져 있으면 빈 스태시로 시작한다.
    pub fn load_from(path: PathBuf) -> Self {
        let entries = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<FileFormat>(&s).ok())
            .map(|f| f.entries)
            .unwrap_or_default();
        Stash { path, entries }
    }

    pub fn save(&self) -> Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
        }
        let data = serde_json::to_string_pretty(&FileFormat { entries: self.entries.clone() })?;
        std::fs::write(&self.path, data).with_context(|| format!("cannot write {}", self.path.display()))?;
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn find(&self, source: &str) -> Option<usize> {
        self.entries.iter().position(|e| e.source == source)
    }

    /// 추가하고 true를 돌려준다. 이미 있으면 메모만 갱신하고 false.
    pub fn add(&mut self, source: &str, note: Option<String>) -> bool {
        if let Some(i) = self.find(source) {
            if note.is_some() {
                self.entries[i].note = note;
            }
            return false;
        }
        let added = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        self.entries.insert(0, Entry { source: source.to_string(), note, added });
        true
    }

    pub fn remove(&mut self, idx: usize) -> Option<Entry> {
        if idx < self.entries.len() { Some(self.entries.remove(idx)) } else { None }
    }

    pub fn set_note(&mut self, idx: usize, note: String) {
        if let Some(e) = self.entries.get_mut(idx) {
            e.note = if note.trim().is_empty() { None } else { Some(note) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("mdview-stash-{}-{name}", std::process::id()))
    }

    /// macOS가 나눠 저장한 파일 이름도 목록에는 합쳐서 보여 준다. 저장 키는 그대로 둔다.
    #[test]
    fn display_name_composes_hangul() {
        let nfd = "/tmp/\u{1112}\u{1161}\u{11ab}\u{1100}\u{1173}\u{11af}.md";
        let e = Entry { source: nfd.to_string(), note: None, added: 0 };
        assert_eq!(e.display_name(), "한글.md");
        assert_eq!(e.source, nfd, "저장된 키는 건드리지 않는다");
    }

    #[test]
    fn add_remove_and_dedupe() {
        let mut s = Stash::load_from(temp_path("a.json"));
        assert!(s.add("/tmp/a.md", None));
        assert!(s.add("https://example.com/x.md", Some("메모".into())));
        assert!(!s.add("/tmp/a.md", Some("new".into())));
        assert_eq!(s.entries.len(), 2);
        // 최근 추가가 앞에 온다
        assert_eq!(s.entries[0].source, "https://example.com/x.md");
        assert_eq!(s.entries[1].note.as_deref(), Some("new"));
        assert!(s.remove(0).is_some());
        assert_eq!(s.entries.len(), 1);
        assert!(s.remove(5).is_none());
    }

    #[test]
    fn roundtrip_to_disk() {
        let p = temp_path("dir/b.json");
        let _ = std::fs::remove_file(&p);
        let mut s = Stash::load_from(p.clone());
        s.add("/tmp/x.md", Some("hello".into()));
        s.save().unwrap();
        let s2 = Stash::load_from(p.clone());
        assert_eq!(s2.entries, s.entries);
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn corrupt_file_yields_empty() {
        let p = temp_path("c.json");
        std::fs::write(&p, "not json").unwrap();
        let s = Stash::load_from(p.clone());
        assert!(s.entries.is_empty());
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn display_name_and_dir() {
        let e = Entry { source: "/home/u/docs/a.md".into(), note: None, added: 0 };
        assert_eq!(e.display_name(), "a.md");
        assert_eq!(e.display_dir(), "/home/u/docs");
        let u = Entry { source: "https://x.y/z.md".into(), note: None, added: 0 };
        assert!(u.is_url());
        assert_eq!(u.display_name(), "https://x.y/z.md");
        assert_eq!(u.display_dir(), "");
    }

    #[test]
    fn set_note_blank_clears() {
        let mut s = Stash::load_from(temp_path("d.json"));
        s.add("/a", Some("n".into()));
        s.set_note(0, "   ".into());
        assert_eq!(s.entries[0].note, None);
        s.set_note(0, "ok".into());
        assert_eq!(s.entries[0].note.as_deref(), Some("ok"));
    }
}
