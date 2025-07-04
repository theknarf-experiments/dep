use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::path::Path;
use vfs::{VfsFileType, VfsPath};

use crate::types::html::HTML_EXTENSIONS;
use crate::types::js::JS_EXTENSIONS;
const SPECIAL_FILES: &[&str] = &["package.json", "pnpm-workspace.yml", "tsconfig.json"];

struct GitIgnoreEntry {
    prefix: String,
    gi: Gitignore,
}

fn load_gitignore(root: &VfsPath) -> anyhow::Result<Vec<GitIgnoreEntry>> {
    let mut entries = Vec::new();
    let walk = match root.walk_dir() {
        Ok(w) => w,
        Err(_) => return Ok(entries),
    };

    let root_str = root.as_str().trim_end_matches('/');

    for entry in walk {
        let path = entry?;
        if path
            .as_str()
            .rsplit_once('/')
            .map(|(_, name)| name == ".gitignore")
            .unwrap_or(false)
        {
            let dir = Path::new(path.as_str()).parent().unwrap_or(Path::new(""));
            let prefix = dir
                .to_str()
                .unwrap_or("")
                .trim_start_matches(root_str)
                .trim_start_matches('/')
                .to_string();
            let contents = path.read_to_string()?;
            let mut builder = GitignoreBuilder::new(dir);
            for line in contents.lines() {
                let _ = builder.add_line(None, line);
            }
            let gi = builder.build()?;
            entries.push(GitIgnoreEntry { prefix, gi });
        }
    }

    Ok(entries)
}

/// Recursively collect JS/TS files starting from `root` respecting `.gitignore`.
pub fn collect_files(root: &VfsPath, color: bool) -> anyhow::Result<Vec<VfsPath>> {
    let gitignore = match load_gitignore(root) {
        Ok(v) => v,
        Err(e) => {
            crate::log_error(color, &format!("failed to read .gitignore: {e}"));
            Vec::new()
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
        let rel = path
            .as_str()
            .strip_prefix(root_str)
            .unwrap_or(path.as_str())
            .trim_start_matches('/');
        if rel == "node_modules"
            || rel.starts_with("node_modules/")
            || rel.contains("/node_modules/")
        {
            continue;
        }
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
        let mut ignored = false;
        for entry in &gitignore {
            if entry.prefix.is_empty()
                || rel == entry.prefix
                || rel.starts_with(&format!("{}/", entry.prefix))
            {
                if entry
                    .gi
                    .matched_path_or_any_parents(Path::new(rel), false)
                    .is_ignore()
                {
                    ignored = true;
                    break;
                }
            }
        }
        if ignored {
            continue;
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
    use proptest::prelude::*;

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

    #[test]
    fn test_recursive_with_gitignore() {
        let fs = TestFS::new([
            (".gitignore", "ignored.js\n"),
            ("foo/a.js", "import '../bar/b.js';\nimport 'fs';"),
            ("bar/b.js", ""),
            ("ignored.js", ""),
        ]);
        let root = fs.root();
        let files = collect_files(&root, false).unwrap();
        assert!(files.iter().all(|p| !p.as_str().ends_with("ignored.js")));
    }

    #[test]
    fn test_skip_node_modules() {
        let fs = TestFS::new([("src/a.js", ""), ("node_modules/b.js", "")]);
        let root = fs.root();
        let files = collect_files(&root, false).unwrap();
        let names: Vec<_> = files
            .iter()
            .map(|p| Path::new(p.as_str()).file_name().unwrap().to_str().unwrap())
            .collect();
        assert!(names.contains(&"a.js"));
        assert!(!names.contains(&"b.js"));
    }

    proptest! {
        #[test]
        fn prop_nested_gitignore(folder in "[a-z]{1,4}", other in "[a-z]{1,4}") {
            prop_assume!(folder != other);
            let entries = vec![
                (".gitignore".to_string(), b"a.tsx\n".to_vec()),
                (format!("{}/.gitignore", folder), b"b.tsx\n".to_vec()),
                (format!("{}/a.tsx", folder), Vec::new()),
                (format!("{}/b.tsx", folder), Vec::new()),
                (format!("{}/c.tsx", folder), Vec::new()),
                (format!("{}/a/a.tsx", folder), Vec::new()),
                (format!("{}/a/b.tsx", folder), Vec::new()),
                (format!("{}/a.tsx", other), Vec::new()),
                (format!("{}/b.tsx", other), Vec::new()),
            ];
            let fs = TestFS::new(entries.iter().map(|(p,c)| (p.as_str(), c.as_slice())));
            let root = fs.root();
            let files = collect_files(&root, false).unwrap();
            let root_str = root.as_str().trim_end_matches('/');
            let names: Vec<String> = files
                .iter()
                .map(|p| p.as_str().strip_prefix(root_str).unwrap_or(p.as_str()).trim_start_matches('/') .to_string())
                .collect();

            let a1 = format!("{}/a.tsx", folder);
            let b1 = format!("{}/b.tsx", folder);
            let c1 = format!("{}/c.tsx", folder);
            let aa1 = format!("{}/a/a.tsx", folder);
            let ab1 = format!("{}/a/b.tsx", folder);
            let a2 = format!("{}/a.tsx", other);
            let b2 = format!("{}/b.tsx", other);

            prop_assert!(!names.contains(&a1));
            prop_assert!(!names.contains(&b1));
            prop_assert!(names.contains(&c1));
            prop_assert!(!names.contains(&aa1));
            prop_assert!(!names.contains(&ab1));
            prop_assert!(!names.contains(&a2));
            prop_assert!(names.contains(&b2));
        }
    }
}
