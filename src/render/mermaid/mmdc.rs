//! mermaid-cli(`mmdc`)로 다이어그램을 PNG로 그린다.
//!
//! 터미널 글자로 그리기엔 너무 넓은 그림을 mermaid.js 그대로의 모양으로
//! 보여주려고 쓴다. `mmdc`가 PATH에 없거나 실패하면 `None`이다.
//! 결과는 `$XDG_CACHE_HOME/mdview/mermaid/`에 원문 해시로 저장해 두고 다시 쓴다.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// 크롬을 띄워 그리므로 넉넉히 기다린다.
const TIMEOUT: Duration = Duration::from_secs(30);
/// 그림 배율. 줄여서 보여줄 때 글자가 뭉개지지 않게 크게 그린다.
const SCALE: &str = "2";

type Memo = Mutex<HashMap<u64, Option<Arc<Vec<u8>>>>>;

/// 이번 실행에서 그려 본 결과(실패 포함). 다시 그릴 때마다 크롬을 띄우지 않으려고 둔다.
fn memo() -> &'static Memo {
    static MEMO: OnceLock<Memo> = OnceLock::new();
    MEMO.get_or_init(|| Mutex::new(HashMap::new()))
}

/// `mmdc` 위치. `$MDVIEW_MMDC` > PATH > npm·nvm이 흔히 까는 곳 순으로 찾는다.
/// nvm은 셸 초기화에서만 PATH에 잡히므로, 데스크톱에서 띄운 경우에도 찾도록 직접 훑는다.
fn mmdc() -> Option<&'static PathBuf> {
    static MMDC: OnceLock<Option<PathBuf>> = OnceLock::new();
    MMDC.get_or_init(|| {
        if let Some(p) = std::env::var_os("MDVIEW_MMDC").map(PathBuf::from) {
            return p.is_file().then_some(p);
        }
        let path = std::env::var_os("PATH").unwrap_or_default();
        std::env::split_paths(&path).chain(fallback_dirs()).map(|d| d.join("mmdc")).find(|p| p.is_file())
    })
    .as_ref()
}

/// PATH 밖에서 `mmdc`가 있을 법한 폴더들. nvm은 새 버전부터 본다.
fn fallback_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut nvm_roots: Vec<PathBuf> = std::env::var_os("NVM_DIR").map(PathBuf::from).into_iter().collect();
    if let Some(h) = &home {
        nvm_roots.push(h.join(".nvm"));
        nvm_roots.push(h.join(".config/nvm"));
    }
    for root in nvm_roots {
        let Ok(rd) = std::fs::read_dir(root.join("versions/node")) else { continue };
        let mut vers: Vec<(Vec<u64>, PathBuf)> = rd
            .flatten()
            .map(|e| {
                let name = e.file_name().to_string_lossy().trim_start_matches('v').to_string();
                (name.split('.').map(|n| n.parse().unwrap_or(0)).collect(), e.path().join("bin"))
            })
            .collect();
        vers.sort_by(|a, b| b.0.cmp(&a.0));
        dirs.extend(vers.into_iter().map(|(_, p)| p));
    }
    if let Some(h) = &home {
        dirs.extend([".npm-global/bin", ".local/bin", ".volta/bin", ".local/share/pnpm"].map(|d| h.join(d)));
    }
    dirs.extend(["/usr/local/bin", "/usr/bin"].map(PathBuf::from));
    dirs
}

/// `mmdc`는 `#!/usr/bin/env node` 스크립트라서, 같은 폴더의 `node`가 보이도록 PATH 앞에 그 폴더를 붙인다.
fn child_path(mmdc: &std::path::Path) -> Option<std::ffi::OsString> {
    let dir = mmdc.parent()?.to_path_buf();
    let rest = std::env::var_os("PATH").unwrap_or_default();
    std::env::join_paths(std::iter::once(dir).chain(std::env::split_paths(&rest))).ok()
}

fn cache_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
    Some(base.join("mdview").join("mermaid"))
}

fn key_of(src: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    SCALE.hash(&mut h);
    src.trim().hash(&mut h);
    h.finish()
}

/// mermaid 원문을 PNG 바이트로. 그릴 수 없으면 `None`.
pub fn png(src: &str) -> Option<Arc<Vec<u8>>> {
    let key = key_of(src);
    if let Some(hit) = memo().lock().ok()?.get(&key) {
        return hit.clone();
    }
    let out = cached_or_draw(src, key).map(Arc::new);
    if let Ok(mut m) = memo().lock() {
        m.insert(key, out.clone());
    }
    out
}

fn cached_or_draw(src: &str, key: u64) -> Option<Vec<u8>> {
    let dir = cache_dir()?;
    let file = dir.join(format!("{key:016x}.png"));
    if let Ok(bytes) = std::fs::read(&file) {
        return Some(bytes);
    }
    let bytes = draw(src, key)?;
    // 캐시에 못 써도 그림은 보여준다. 반쯤 쓴 파일이 남지 않게 옮겨 놓는다.
    if std::fs::create_dir_all(&dir).is_ok() {
        let tmp = dir.join(format!("{key:016x}.{}.tmp", std::process::id()));
        if std::fs::write(&tmp, &bytes).is_ok() && std::fs::rename(&tmp, &file).is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
    }
    Some(bytes)
}

/// `mmdc`를 실행해 그린다.
fn draw(src: &str, key: u64) -> Option<Vec<u8>> {
    let mmdc = mmdc()?;
    let work = std::env::temp_dir().join(format!("mdview-mermaid-{}-{key:016x}", std::process::id()));
    std::fs::create_dir_all(&work).ok()?;
    let input = work.join("in.mmd");
    let output = work.join("out.png");
    let result = (|| {
        std::fs::write(&input, src).ok()?;
        let mut cmd = Command::new(mmdc);
        if let Some(path) = child_path(mmdc) {
            cmd.env("PATH", path);
        }
        let mut child = cmd
            .arg("-i")
            .arg(&input)
            .arg("-o")
            .arg(&output)
            .args(["-t", "neutral", "-b", "white", "-s", SCALE, "-q"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let start = Instant::now();
        loop {
            match child.try_wait().ok()? {
                Some(status) if status.success() => break,
                Some(_) => return None,
                None if start.elapsed() > TIMEOUT => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                None => std::thread::sleep(Duration::from_millis(30)),
            }
        }
        std::fs::read(&output).ok().filter(|b| !b.is_empty())
    })();
    let _ = std::fs::remove_dir_all(&work);
    result
}
