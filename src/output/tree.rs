//! 파일 목록을 계층 구조로 펼치는 순수 함수들.
//!
//! 목록 위젯은 평면 배열만 그릴 수 있으므로, 디렉터리 트리를 미리 "행"으로
//! 펼쳐 둔다. 접힌 디렉터리의 하위는 행에서 빠진다.

use std::collections::BTreeMap;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// 목록에 그려질 한 줄.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row {
    /// 파일. `idx`는 원본 목록에서의 위치.
    Item { idx: usize, depth: usize },
    /// 디렉터리(트리 보기에서만 나온다).
    Dir { path: PathBuf, depth: usize, open: bool, files: usize },
}

impl Row {
    pub fn depth(&self) -> usize {
        match self {
            Row::Item { depth, .. } | Row::Dir { depth, .. } => *depth,
        }
    }
    pub fn item_index(&self) -> Option<usize> {
        match self {
            Row::Item { idx, .. } => Some(*idx),
            Row::Dir { .. } => None,
        }
    }
}

#[derive(Default)]
struct Node {
    dirs: BTreeMap<String, Node>,
    /// (원본 인덱스, 파일 이름)
    files: Vec<(usize, String)>,
}

impl Node {
    fn insert(&mut self, idx: usize, parts: &[String]) {
        match parts {
            [] => {}
            [name] => self.files.push((idx, name.clone())),
            [head, rest @ ..] => self.dirs.entry(head.clone()).or_default().insert(idx, rest),
        }
    }

    fn file_count(&self) -> usize {
        self.files.len() + self.dirs.values().map(Node::file_count).sum::<usize>()
    }
}

/// 상대 경로 목록을 계층 행으로 펼친다.
///
/// 각 단계에서 디렉터리를 먼저, 그다음 파일을 이름순으로 늘어놓는다.
/// `collapsed`에 든 디렉터리는 하위를 펼치지 않는다.
pub fn rows(files: &[(usize, PathBuf)], collapsed: &HashSet<PathBuf>) -> Vec<Row> {
    let mut root = Node::default();
    for (idx, path) in files {
        let parts: Vec<String> = path.iter().map(|c| c.to_string_lossy().into_owned()).collect();
        root.insert(*idx, &parts);
    }
    let mut out = Vec::new();
    walk(&root, Path::new(""), 0, collapsed, &mut out);
    out
}

fn walk(node: &Node, prefix: &Path, depth: usize, collapsed: &HashSet<PathBuf>, out: &mut Vec<Row>) {
    for (name, child) in &node.dirs {
        let path = prefix.join(name);
        let open = !collapsed.contains(&path);
        out.push(Row::Dir { path: path.clone(), depth, open, files: child.file_count() });
        if open {
            walk(child, &path, depth + 1, collapsed, out);
        }
    }
    let mut files = node.files.clone();
    files.sort_by(|a, b| a.1.cmp(&b.1));
    for (idx, _) in files {
        out.push(Row::Item { idx, depth });
    }
}

/// 평면 보기용 행.
pub fn flat_rows(files: &[(usize, PathBuf)]) -> Vec<Row> {
    files.iter().map(|(idx, _)| Row::Item { idx: *idx, depth: 0 }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(list: &[&str]) -> Vec<(usize, PathBuf)> {
        list.iter().enumerate().map(|(i, p)| (i, PathBuf::from(p))).collect()
    }

    fn shape(rows: &[Row]) -> Vec<String> {
        rows.iter()
            .map(|r| match r {
                Row::Dir { path, depth, open, files } => {
                    format!("{}{}/ ({files}){}", "  ".repeat(*depth), path.file_name().unwrap().to_string_lossy(), if *open { "" } else { " [접힘]" })
                }
                Row::Item { idx, depth } => format!("{}#{idx}", "  ".repeat(*depth)),
            })
            .collect()
    }

    #[test]
    fn groups_files_under_directories() {
        let files = paths(&["README.md", "docs/guide.md", "docs/api/ref.md"]);
        let out = rows(&files, &HashSet::new());
        assert_eq!(shape(&out), vec!["docs/ (2)", "  api/ (1)", "    #2", "  #1", "#0"]);
    }

    #[test]
    fn directories_come_before_files_at_each_level() {
        let files = paths(&["a.md", "sub/b.md"]);
        let out = rows(&files, &HashSet::new());
        assert_eq!(shape(&out), vec!["sub/ (1)", "  #1", "#0"]);
    }

    #[test]
    fn collapsed_directory_hides_children() {
        let files = paths(&["docs/guide.md", "docs/api/ref.md", "top.md"]);
        let collapsed: HashSet<PathBuf> = [PathBuf::from("docs")].into_iter().collect();
        let out = rows(&files, &collapsed);
        assert_eq!(shape(&out), vec!["docs/ (2) [접힘]", "#2"]);
    }

    #[test]
    fn nested_collapse_only_hides_that_subtree() {
        let files = paths(&["docs/guide.md", "docs/api/ref.md"]);
        let collapsed: HashSet<PathBuf> = [PathBuf::from("docs/api")].into_iter().collect();
        let out = rows(&files, &collapsed);
        assert_eq!(shape(&out), vec!["docs/ (2)", "  api/ (1) [접힘]", "  #0"]);
    }

    #[test]
    fn flat_view_keeps_original_order() {
        let files = paths(&["z.md", "a/b.md"]);
        let out = flat_rows(&files);
        assert_eq!(out, vec![Row::Item { idx: 0, depth: 0 }, Row::Item { idx: 1, depth: 0 }]);
    }

    #[test]
    fn empty_input_yields_no_rows() {
        assert!(rows(&[], &HashSet::new()).is_empty());
        assert!(flat_rows(&[]).is_empty());
    }

    #[test]
    fn files_are_sorted_within_a_directory() {
        let files = paths(&["b.md", "a.md"]);
        let out = rows(&files, &HashSet::new());
        assert_eq!(out, vec![Row::Item { idx: 1, depth: 0 }, Row::Item { idx: 0, depth: 0 }]);
    }
}
