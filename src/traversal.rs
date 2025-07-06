use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::path::Path;
use vfs::{VfsFileType, VfsPath};

use crate::{LogLevel, Logger};

fn load_gitignore(root: &VfsPath) -> anyhow::Result<Option<Gitignore>> {
    if let Ok(path) = root.join(".gitignore") {
        if path.exists()? {
            let contents = path.read_to_string()?;
            let mut builder = GitignoreBuilder::new("");
            for line in contents.lines() {
                let _ = builder.add_line(None, line);
            }
            let gi = builder.build()?;
            return Ok(Some(gi));
        }
    }
    Ok(None)
}

/// Recursively collect all files starting from `root` while respecting `.gitignore`.

pub fn collect_files(root: &VfsPath, logger: &dyn Logger) -> anyhow::Result<Vec<VfsPath>> {
    let gitignore = match load_gitignore(root) {
        Ok(v) => v,
        Err(e) => {
            logger.log(LogLevel::Error, &format!("failed to read .gitignore: {e}"));
            None
        }
    };
    let root_str = root.as_str().trim_end_matches('/');
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
        if let Some(gi) = &gitignore {
            if gi
                .matched_path_or_any_parents(Path::new(rel), false)
                .is_ignore()
            {
                continue;
            }
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
}
