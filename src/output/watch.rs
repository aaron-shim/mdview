//! 파일 브라우저 목록 자동 갱신.
//!
//! 백그라운드 스레드가 루트 폴더를 주기적으로 다시 훑어, 파일 목록이나
//! 수정 시각·크기가 달라졌을 때만 스냅숏을 보낸다. inotify 같은 OS 기능을 쓰지
//! 않아 macOS·Linux에서 똑같이 동작하고, 감시 개수 제한에도 걸리지 않는다.
//!
//! 훑는 데 오래 걸리는 넓은 폴더에서는 간격을 스스로 늘려 CPU를 잡아먹지 않는다.

use crate::source::{self, Stamp};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

/// 한 번 훑은 결과.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub root: PathBuf,
    pub files: Vec<(PathBuf, Stamp)>,
}

/// 두 스냅숏 사이의 차이. 경로는 루트 기준 상대 경로.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Diff {
    pub added: Vec<PathBuf>,
    pub modified: Vec<PathBuf>,
    pub removed: Vec<PathBuf>,
}

impl Diff {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.modified.is_empty() && self.removed.is_empty()
    }

    /// 푸터에 띄울 한 줄 요약.
    pub fn summary(&self) -> String {
        let name = |p: &Path| crate::hangul::compose(&p.to_string_lossy());
        // 하나만 바뀌었으면 파일 이름을 보여 주는 편이 알아보기 쉽다.
        match (self.added.len(), self.modified.len(), self.removed.len()) {
            (1, 0, 0) => format!("새 파일: {}", name(&self.added[0])),
            (0, 1, 0) => format!("변경됨: {}", name(&self.modified[0])),
            (0, 0, 1) => format!("삭제됨: {}", name(&self.removed[0])),
            (a, m, r) => {
                let mut parts = Vec::new();
                if a > 0 {
                    parts.push(format!("추가 {a}"));
                }
                if m > 0 {
                    parts.push(format!("변경 {m}"));
                }
                if r > 0 {
                    parts.push(format!("삭제 {r}"));
                }
                format!("목록 갱신: {}", parts.join(", "))
            }
        }
    }
}

/// 이전·현재 파일 목록을 비교한다.
pub fn diff(old: &[(PathBuf, Stamp)], new: &[(PathBuf, Stamp)]) -> Diff {
    use std::collections::HashMap;
    let before: HashMap<&PathBuf, &Stamp> = old.iter().map(|(p, s)| (p, s)).collect();
    let after: HashMap<&PathBuf, &Stamp> = new.iter().map(|(p, s)| (p, s)).collect();
    let mut d = Diff::default();
    for (p, s) in new {
        match before.get(p) {
            None => d.added.push(p.clone()),
            Some(prev) if *prev != s => d.modified.push(p.clone()),
            _ => {}
        }
    }
    for (p, _) in old {
        if !after.contains_key(p) {
            d.removed.push(p.clone());
        }
    }
    d
}

/// 가장 짧은/긴 재검사 간격.
const MIN_INTERVAL: Duration = Duration::from_millis(1000);
const MAX_INTERVAL: Duration = Duration::from_secs(15);

/// 훑는 데 걸린 시간에 비례해 다음 검사까지 쉰다.
/// 작은 프로젝트는 1초마다, 홈 디렉터리처럼 넓으면 느긋하게.
pub fn next_interval(scan_took: Duration) -> Duration {
    (scan_took * 8).clamp(MIN_INTERVAL, MAX_INTERVAL)
}

pub struct Watcher {
    ctrl: Sender<PathBuf>,
    rx: Receiver<Snapshot>,
}

impl Watcher {
    /// `initial`은 이미 화면에 보여 준 목록. 같으면 다시 보내지 않는다.
    pub fn spawn(root: PathBuf, initial: Vec<(PathBuf, Stamp)>) -> Self {
        let (ctrl, ctrl_rx) = mpsc::channel::<PathBuf>();
        let (tx, rx) = mpsc::channel::<Snapshot>();
        std::thread::Builder::new()
            .name("mdview-watch".into())
            .spawn(move || run(root, initial, ctrl_rx, tx))
            .expect("감시 스레드를 만들 수 없습니다");
        Watcher { ctrl, rx }
    }

    /// 감시할 루트를 바꾸고 곧바로 훑게 한다.
    pub fn set_root(&self, root: PathBuf) {
        let _ = self.ctrl.send(root);
    }

    /// 쌓인 스냅숏 중 가장 최근 것만 꺼낸다.
    pub fn latest(&self) -> Option<Snapshot> {
        let mut last = None;
        while let Ok(s) = self.rx.try_recv() {
            last = Some(s);
        }
        last
    }
}

fn run(mut root: PathBuf, initial: Vec<(PathBuf, Stamp)>, ctrl: Receiver<PathBuf>, tx: Sender<Snapshot>) {
    let mut last: Option<Vec<(PathBuf, Stamp)>> = Some(initial);
    let mut wait = MIN_INTERVAL;
    loop {
        match ctrl.recv_timeout(wait) {
            Ok(new_root) => {
                // 루트가 바뀌면 비교 기준도 버리고 즉시 훑는다.
                root = new_root;
                last = None;
            }
            Err(RecvTimeoutError::Timeout) => {}
            // Watcher 가 사라지면 스레드도 끝낸다.
            Err(RecvTimeoutError::Disconnected) => return,
        }
        let started = Instant::now();
        let files = source::scan_markdown_files(&root);
        wait = next_interval(started.elapsed());
        if last.as_ref() != Some(&files) {
            last = Some(files.clone());
            if tx.send(Snapshot { root: root.clone(), files }).is_err() {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

    fn st(secs: u64, len: u64) -> Stamp {
        (Some(SystemTime::UNIX_EPOCH + Duration::from_secs(secs)), len)
    }

    fn f(p: &str, s: Stamp) -> (PathBuf, Stamp) {
        (PathBuf::from(p), s)
    }

    #[test]
    fn diff_finds_added_modified_removed() {
        let old = vec![f("a.md", st(1, 10)), f("b.md", st(1, 10)), f("gone.md", st(1, 1))];
        let new = vec![f("a.md", st(1, 10)), f("b.md", st(2, 12)), f("new.md", st(3, 5))];
        let d = diff(&old, &new);
        assert_eq!(d.added, vec![PathBuf::from("new.md")]);
        assert_eq!(d.modified, vec![PathBuf::from("b.md")]);
        assert_eq!(d.removed, vec![PathBuf::from("gone.md")]);
    }

    #[test]
    fn same_size_but_newer_time_counts_as_modified() {
        let d = diff(&[f("a.md", st(1, 10))], &[f("a.md", st(5, 10))]);
        assert_eq!(d.modified, vec![PathBuf::from("a.md")]);
    }

    #[test]
    fn identical_lists_have_no_diff() {
        let list = vec![f("a.md", st(1, 10))];
        assert!(diff(&list, &list).is_empty());
    }

    #[test]
    fn summary_names_a_single_change() {
        let d = Diff { added: vec![PathBuf::from("docs/새 문서.md")], ..Default::default() };
        assert_eq!(d.summary(), "새 파일: docs/새 문서.md");
        let d = Diff { added: vec!["a".into(), "b".into()], removed: vec!["c".into()], ..Default::default() };
        assert_eq!(d.summary(), "목록 갱신: 추가 2, 삭제 1");
    }

    #[test]
    fn interval_grows_with_scan_cost_within_bounds() {
        assert_eq!(next_interval(Duration::from_millis(5)), MIN_INTERVAL);
        assert_eq!(next_interval(Duration::from_millis(500)), Duration::from_secs(4));
        assert_eq!(next_interval(Duration::from_secs(10)), MAX_INTERVAL);
    }

    #[test]
    fn watcher_reports_new_and_changed_files() {
        let dir = std::env::temp_dir().join(format!("mdview-watch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.md"), "# a").unwrap();
        let initial = source::scan_markdown_files(&dir);
        let w = Watcher::spawn(dir.clone(), initial.clone());

        std::fs::write(dir.join("b.md"), "# b").unwrap();
        let snap = wait_for(&w, |s| s.files.len() == 2).expect("새 파일을 알아채야 한다");
        assert_eq!(diff(&initial, &snap.files).added, vec![PathBuf::from("b.md")]);

        std::fs::write(dir.join("a.md"), "# a 내용이 길어졌다").unwrap();
        let snap2 = wait_for(&w, |s| diff(&snap.files, &s.files).modified.contains(&PathBuf::from("a.md"))).expect("내용 변경을 알아채야 한다");
        assert_eq!(snap2.root, dir);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn watcher_follows_a_new_root_immediately() {
        let base = std::env::temp_dir().join(format!("mdview-watch-root-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("child")).unwrap();
        std::fs::write(base.join("top.md"), "x").unwrap();
        std::fs::write(base.join("child/in.md"), "x").unwrap();
        let child = base.join("child");
        let w = Watcher::spawn(child.clone(), source::scan_markdown_files(&child));
        w.set_root(base.clone());
        let snap = wait_for(&w, |s| s.root == base).expect("루트를 바꾸면 곧바로 새 스냅숏이 와야 한다");
        let names: Vec<_> = snap.files.iter().map(|(p, _)| p.clone()).collect();
        assert_eq!(names, vec![PathBuf::from("top.md"), PathBuf::from("child/in.md")]);
        let _ = std::fs::remove_dir_all(&base);
    }

    fn wait_for(w: &Watcher, ok: impl Fn(&Snapshot) -> bool) -> Option<Snapshot> {
        let deadline = Instant::now() + Duration::from_secs(6);
        while Instant::now() < deadline {
            if let Some(s) = w.latest()
                && ok(&s)
            {
                return Some(s);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        None
    }
}
