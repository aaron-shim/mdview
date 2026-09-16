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
    /// 이 결과를 모을 때 숨김 파일을 포함했는지. 설정을 바꾸기 전에 출발한 결과를 걸러내는 데 쓴다.
    pub hidden: bool,
    pub files: Vec<(PathBuf, Stamp)>,
    /// `false`면 아직 훑는 중에 보낸 부분 결과. 루트를 막 바꿨을 때만 온다.
    pub done: bool,
}

/// 감시기에게 보내는 주문: 이 폴더를 이 설정으로 훑어라.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub root: PathBuf,
    pub hidden: bool,
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
    ctrl: Sender<Request>,
    rx: Receiver<Snapshot>,
}

impl Watcher {
    /// `initial`은 이미 화면에 보여 준 목록. 같으면 다시 보내지 않는다.
    pub fn spawn(root: PathBuf, hidden: bool, initial: Vec<(PathBuf, Stamp)>) -> Self {
        let (ctrl, ctrl_rx) = mpsc::channel::<Request>();
        let (tx, rx) = mpsc::channel::<Snapshot>();
        std::thread::Builder::new()
            .name("mdview-watch".into())
            .spawn(move || run(Request { root, hidden }, initial, ctrl_rx, tx))
            .expect("감시 스레드를 만들 수 없습니다");
        Watcher { ctrl, rx }
    }

    /// 훑을 폴더나 숨김 설정을 바꾸고 곧바로 다시 훑게 한다.
    pub fn request(&self, root: PathBuf, hidden: bool) {
        let _ = self.ctrl.send(Request { root, hidden });
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

fn run(mut req: Request, initial: Vec<(PathBuf, Stamp)>, ctrl: Receiver<Request>, tx: Sender<Snapshot>) {
    let mut last: Option<Vec<(PathBuf, Stamp)>> = Some(initial);
    let mut wait = MIN_INTERVAL;
    // 훑는 도중 들어온 새 주문. 쉬지 않고 바로 그쪽을 훑는다.
    let mut pending: Option<Request> = None;
    loop {
        let newest = match pending.take() {
            Some(r) => Some(r),
            None => match ctrl.recv_timeout(wait) {
                Ok(r) => Some(r),
                Err(RecvTimeoutError::Timeout) => None,
                // Watcher 가 사라지면 스레드도 끝낸다.
                Err(RecvTimeoutError::Disconnected) => return,
            },
        };
        // 연달아 들어온 주문은 마지막 것만 훑는다.
        let newest = ctrl.try_iter().last().or(newest);
        // 루트를 막 바꿨을 때는 화면이 비어 있으니 모이는 대로 부분 결과를 보낸다.
        // 같은 폴더에서 숨김만 켜고 끈 경우에는 보던 목록이 그대로 있으니 완료본만 보낸다.
        let mut stream = false;
        if let Some(r) = newest {
            stream = r.root != req.root;
            req = r;
            last = None;
        }
        let mut sent = 0;
        let mut gone = false;
        let started = Instant::now();
        let files = source::scan_markdown_files_with(&req.root, req.hidden, |partial| {
            if let Some(r) = ctrl.try_iter().last() {
                pending = Some(r);
                return false;
            }
            if stream && partial.len() > sent {
                sent = partial.len();
                if tx.send(Snapshot { root: req.root.clone(), hidden: req.hidden, files: partial.to_vec(), done: false }).is_err() {
                    gone = true;
                    return false;
                }
            }
            true
        });
        if gone {
            return;
        }
        let Some(files) = files else { continue };
        wait = next_interval(started.elapsed());
        if last.as_ref() != Some(&files) {
            last = Some(files.clone());
            if tx.send(Snapshot { root: req.root.clone(), hidden: req.hidden, files, done: true }).is_err() {
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
        let initial = source::scan_markdown_files(&dir, false);
        let w = Watcher::spawn(dir.clone(), false, initial.clone());

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
        let w = Watcher::spawn(child.clone(), false, source::scan_markdown_files(&child, false));
        w.request(base.clone(), false);
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

    fn big_dir(name: &str, n: usize) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mdview-watch-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for i in 0..n {
            std::fs::write(dir.join(format!("f{i:04}.md")), "x").unwrap();
        }
        dir
    }

    /// `latest`와 달리 온 순서대로 모두 모은다.
    fn collect(w: &Watcher, until: impl Fn(&[Snapshot]) -> bool, limit: Duration) -> Vec<Snapshot> {
        let deadline = Instant::now() + limit;
        let mut got = Vec::new();
        while Instant::now() < deadline && !until(&got) {
            while let Ok(s) = w.rx.try_recv() {
                got.push(s);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        got
    }

    #[test]
    fn a_new_root_streams_partial_snapshots_before_the_final_one() {
        let big = big_dir("stream", source::PROGRESS_EVERY * 3 + 1);
        let small = big_dir("stream-small", 1);
        let w = Watcher::spawn(small.clone(), false, source::scan_markdown_files(&small, false));
        w.request(big.clone(), false);
        let got = collect(&w, |g| g.iter().any(|s| s.root == big && s.done), Duration::from_secs(6));
        let partials: Vec<_> = got.iter().filter(|s| s.root == big && !s.done).collect();
        let last = got.iter().rfind(|s| s.root == big && s.done).expect("완료 스냅숏이 온다");
        assert!(!partials.is_empty(), "완료 전에 부분 스냅숏이 먼저 온다");
        assert!(partials.iter().all(|p| p.files.len() < last.files.len()));
        assert_eq!(last.files.len(), source::PROGRESS_EVERY * 3 + 1);
        let _ = std::fs::remove_dir_all(&big);
        let _ = std::fs::remove_dir_all(&small);
    }

    #[test]
    fn a_newer_root_cancels_the_scan_of_the_previous_one() {
        let a = big_dir("stale-a", 3000);
        let b = big_dir("stale-b", 2);
        let start = big_dir("stale-start", 1);
        let w = Watcher::spawn(start.clone(), false, source::scan_markdown_files(&start, false));
        w.request(a.clone(), false);
        w.request(b.clone(), false);
        let got = collect(&w, |g| g.iter().any(|s| s.root == b && s.done), Duration::from_secs(6));
        assert!(got.iter().any(|s| s.root == b && s.done), "마지막 루트의 완료 스냅숏이 온다");
        assert!(!got.iter().any(|s| s.root == a && s.done), "취소된 루트의 완료 스냅숏은 오지 않는다");
        for d in [&a, &b, &start] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    #[test]
    fn turning_hidden_on_rescans_the_same_root_without_partial_snapshots() {
        let dir = std::env::temp_dir().join(format!("mdview-watch-hidden-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".notes")).unwrap();
        std::fs::write(dir.join("a.md"), "x").unwrap();
        std::fs::write(dir.join(".notes/secret.md"), "x").unwrap();
        let w = Watcher::spawn(dir.clone(), false, source::scan_markdown_files(&dir, false));

        w.request(dir.clone(), true);
        let got = collect(&w, |g| g.iter().any(|s| s.hidden && s.done), Duration::from_secs(6));
        let snap = got.iter().rfind(|s| s.hidden && s.done).expect("숨김을 켜면 다시 훑은 결과가 온다");
        assert!(snap.files.iter().any(|(p, _)| p == Path::new(".notes/secret.md")));
        assert!(got.iter().all(|s| s.done), "같은 폴더에서 설정만 바뀐 것이니 부분 결과는 보내지 않는다");

        w.request(dir.clone(), false);
        let got = collect(&w, |g| g.iter().any(|s| !s.hidden && s.done), Duration::from_secs(6));
        let snap = got.iter().rfind(|s| !s.hidden && s.done).expect("숨김을 끄면 다시 훑은 결과가 온다");
        assert!(!snap.files.iter().any(|(p, _)| p.starts_with(".notes")), "숨김 폴더가 목록에서 빠진다");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn routine_rescans_send_only_complete_snapshots() {
        let dir = big_dir("routine", source::PROGRESS_EVERY * 2 + 1);
        let w = Watcher::spawn(dir.clone(), false, source::scan_markdown_files(&dir, false));
        std::fs::write(dir.join("new.md"), "x").unwrap();
        let got = collect(&w, |g| g.iter().any(|s| s.done), Duration::from_secs(6));
        assert!(got.iter().all(|s| s.done), "같은 루트를 다시 훑을 때는 완료본만 보낸다");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
