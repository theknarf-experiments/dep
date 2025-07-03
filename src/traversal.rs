use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::path::Path;
use vfs::{VfsFileType, VfsPath};

use crate::{HTML_EXTENSIONS, JS_EXTENSIONS};
const SPECIAL_FILES: &[&str] = &["package.json", "pnpm-workspace.yml", "tsconfig.json"];

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

/// Recursively collect JS/TS files starting from `root` respecting `.gitignore`.
pub fn collect_files(root: &VfsPath, color: bool) -> anyhow::Result<Vec<VfsPath>> {
    let gitignore = match load_gitignore(root) {
        Ok(v) => v,
        Err(e) => {
            crate::log_error(color, &format!("failed to read .gitignore: {e}"));
            None
        }
    };
    let root_str = root.as_str().trim_end_matches('/');
    let mut files = Vec::new();
    let walk = match root.walk_dir() {
        Ok(w) => w,
        Err(e) => {
            crate::log_error(color, &format!("failed to walk {}: {e}", root.as_str()));
            return Ok(files);
        }
    };
    for entry in walk {
        let path = match entry {
            Ok(p) => p,
            Err(e) => {
                crate::log_error(color, &format!("walk error: {e}"));
                continue;
            }
        };
        let meta = match path.metadata() {
            Ok(m) => m,
            Err(e) => {
                crate::log_error(color, &format!("metadata error on {}: {e}", path.as_str()));
                continue;
            }
        };
        if meta.file_type != VfsFileType::File {
            continue;
        }
        let rel = path
            .as_str()
            .strip_prefix(root_str)
            .unwrap_or(path.as_str())
            .trim_start_matches('/');
        if let Some(gi) = &gitignore {
            if gi
                .matched_path_or_any_parents(Path::new(rel), false)
                .is_ignore()
            {
                continue;
            }
        }
        let name = Path::new(path.as_str())
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        let ext = Path::new(path.as_str())
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        if JS_EXTENSIONS.contains(&ext)
            || HTML_EXTENSIONS.contains(&ext)
            || SPECIAL_FILES.contains(&name)
        {
            files.push(path);
        }
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
        let files = collect_files(&root, false).unwrap();
        let names: Vec<_> = files
            .iter()
            .map(|p| Path::new(p.as_str()).file_name().unwrap().to_str().unwrap())
            .collect();
        assert!(names.contains(&"a.js"));
        assert!(names.contains(&"c.ts"));
        assert!(!names.contains(&"b.js"));
    }

    #[test]
    fn test_collect_files_missing_dir() {
        let fs = TestFS::new([] as [(&str, &str); 0]);
        let root = fs.root();
        let missing = root.join("missing").unwrap();
        let files = collect_files(&missing, false).unwrap();
        assert!(files.is_empty());
    }
}
