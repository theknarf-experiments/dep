use bstr::ByteSlice;
use gix_ignore::{glob::pattern::Case, search::Match, Search};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use vfs::{VfsFileType, VfsPath};

use crate::{LogLevel, Logger};

/// Recursively collect all files starting from `root` while respecting `.gitignore`.

pub fn collect_files(root: &VfsPath, logger: &dyn Logger) -> anyhow::Result<Vec<VfsPath>> {
    let root_str = root.as_str().trim_end_matches('/');
    let root_path = if root_str.is_empty() { Path::new("/") } else { Path::new(root_str) };

    let mut search = Search::default();
    let mut visited_dirs: HashSet<String> = HashSet::new();

    fn ignored(search: &Search, mut rel: &str, mut is_dir: bool) -> bool {
        loop {
            if let Some(Match { pattern, .. }) =
                search.pattern_matching_relative_path(rel.as_bytes().as_bstr(), Some(is_dir), Case::Sensitive)
            {
                return !pattern.is_negative();
            }
            if let Some(pos) = rel.rfind('/') {
                rel = &rel[..pos];
                is_dir = true;
            } else {
                break;
            }
        }
        false
    }

    // Load root .gitignore if present
    if let Ok(gi_path) = root.join(".gitignore") {
        if gi_path.exists().unwrap_or(false) {
            if let Ok(contents) = gi_path.read_to_string() {
                search.add_patterns_buffer(contents.as_bytes(), PathBuf::from(gi_path.as_str()), Some(root_path));
            }
        }
    }

    let mut files = Vec::new();
    let walk = match root.walk_dir() {
        Ok(w) => w,
        Err(e) => {
            logger.log(LogLevel::Error, &format!("failed to walk {}: {e}", root.as_str()));
            return Ok(files);
        }
    };
    for entry in walk {
        let path = match entry {
            Ok(p) => p,
            Err(e) => {
                logger.log(LogLevel::Error, &format!("walk error: {e}"));
                continue;
            }
        };

        let parent = path.parent();
        if visited_dirs.insert(parent.as_str().to_string()) {
            if let Ok(gi) = parent.join(".gitignore") {
                if gi.exists().unwrap_or(false) {
                    if let Ok(contents) = gi.read_to_string() {
                        search.add_patterns_buffer(contents.as_bytes(), PathBuf::from(gi.as_str()), Some(root_path));
                    }
                }
            }
        }

        let rel = path
            .as_str()
            .strip_prefix(root_str)
            .unwrap_or(path.as_str())
            .trim_start_matches('/');

        let meta = match path.metadata() {
            Ok(m) => m,
            Err(e) => {
                logger.log(LogLevel::Error, &format!("metadata error on {}: {e}", path.as_str()));
                continue;
            }
        };

        if meta.file_type != VfsFileType::File {
            continue;
        }

        if ignored(&search, rel, false) {
            continue;
        }

        files.push(path);
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::TestFS;

    #[test]
    fn test_collect_files_respects_gitignore() {
        let fs = TestFS::new([
            (".gitignore", "b.js\n"),
            ("a.js", ""),
            ("b.js", ""),
            ("sub/c.ts", ""),
        ]);
        let root = fs.root();
        let logger = crate::EmptyLogger;
        let files = collect_files(&root, &logger).unwrap();
        let names: Vec<_> = files
            .iter()
            .map(|p| Path::new(p.as_str()).file_name().unwrap().to_str().unwrap())
            .collect();
        assert!(names.contains(&"a.js"));
        assert!(names.contains(&"c.ts"));
        // .gitignore should not include b.js
        assert!(!names.contains(&"b.js"));
        // The .gitignore file itself is included since it is not ignored
        assert!(names.contains(&".gitignore"));
    }

    #[test]
    fn test_collect_files_missing_dir() {
        let fs = TestFS::new([] as [(&str, &str); 0]);
        let root = fs.root();
        let missing = root.join("missing").unwrap();
        let logger = crate::EmptyLogger;
        let files = collect_files(&missing, &logger).unwrap();
        assert!(files.is_empty());
    }

    #[test]
    fn test_recursive_with_gitignore() {
        let fs = TestFS::new([
            (".gitignore", "ignored.js\n"),
            ("foo/a.js", "import '../bar/b.js';\nimport 'fs';"),
            ("bar/b.js", ""),
            ("ignored.js", ""),
        ]);
        let root = fs.root();
        let logger = crate::EmptyLogger;
        let files = collect_files(&root, &logger).unwrap();
        assert!(files.iter().all(|p| !p.as_str().ends_with("ignored.js")));
    }

    #[test]
    fn test_gitignore_node_modules() {
        let fs = TestFS::new([
            (".gitignore", "node_modules/\n"),
            ("src/a.js", ""),
            ("node_modules/b.js", ""),
            ("node_modules/sub/c.js", ""),
        ]);
        let root = fs.root();
        let logger = crate::EmptyLogger;
        let files = collect_files(&root, &logger).unwrap();
        let names: Vec<_> = files
            .iter()
            .map(|p| Path::new(p.as_str()).file_name().unwrap().to_str().unwrap())
            .collect();
        assert!(names.contains(&"a.js"));
        assert!(names.contains(&".gitignore"));
        assert!(!names.contains(&"b.js"));
        assert!(!names.contains(&"c.js"));
    }

    #[test]
    fn test_nested_gitignore() {
        let fs = TestFS::new([
            (".gitignore", "/a.js\n"),
            ("a.js", ""),
            ("b.js", ""),
            ("sub/.gitignore", "b.js\n"),
            ("sub/a.js", ""),
            ("sub/b.js", ""),
            ("sub/c.js", ""),
        ]);
        let root = fs.root();
        let logger = crate::EmptyLogger;
        let files = collect_files(&root, &logger).unwrap();
        let paths: Vec<_> = files.iter().map(|p| p.as_str()).collect();
        assert!(paths.contains(&"/b.js"));
        assert!(paths.contains(&"/.gitignore"));
        assert!(paths.contains(&"/sub/.gitignore"));
        assert!(paths.contains(&"/sub/a.js"));
        assert!(paths.contains(&"/sub/c.js"));
        assert!(!paths.contains(&"/a.js"));
        assert!(!paths.contains(&"/sub/b.js"));
    }
}
