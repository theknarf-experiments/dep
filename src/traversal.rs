use bstr::ByteSlice;
use gix_ignore::{glob::pattern::Case, search::Match, Search};
use std::path::{Path, PathBuf};
use vfs::{VfsFileType, VfsPath};

use crate::{LogLevel, Logger};

/// Builder for creating [`Walk`] instances.
pub struct WalkBuilder<'a> {
    root: &'a VfsPath,
    patterns: Vec<String>,
}

impl<'a> WalkBuilder<'a> {
    /// Create a new builder with the given root directory.
    pub fn new(root: &'a VfsPath) -> Self {
        Self {
            root,
            // ignore .git folders by default
            patterns: vec![".git/".to_string()],
        }
    }

    /// Add ignore patterns. Patterns use gitignore syntax.
    pub fn ignore_patterns(mut self, pats: &[String]) -> Self {
        self.patterns.extend_from_slice(pats);
        self
    }

    /// Build a [`Walk`] using the collected options.
    pub fn build(self) -> Walk<'a> {
        Walk {
            root: self.root,
            patterns: self.patterns,
        }
    }
}

/// File tree walker that respects `.gitignore`, `.git` folders and custom
/// ignore patterns.
pub struct Walk<'a> {
    root: &'a VfsPath,
    patterns: Vec<String>,
}

impl<'a> Walk<'a> {
    /// Return the root path for this walk.
    pub fn root(&self) -> &VfsPath {
        self.root
    }

    /// Recursively collect all files starting from the walk root while
    /// respecting `.gitignore`, `.git` folders and the configured ignore
    /// patterns.
    pub fn collect_files(&self, logger: &dyn Logger) -> anyhow::Result<Vec<VfsPath>> {
        let root = self.root;
        let patterns = &self.patterns;

        let root_str = root.as_str().trim_end_matches('/');
        let root_path = if root_str.is_empty() {
            Path::new("/")
        } else {
            Path::new(root_str)
        };

        let mut search = Search::default();

        for pat in patterns {
            let buf = format!("{}\n", pat);
            search.add_patterns_buffer(buf.as_bytes(), root_path.join("_cli_ignore"), Some(root_path));
        }

        fn ignored(search: &Search, mut rel: &str, mut is_dir: bool) -> bool {
            loop {
                if let Some(Match { pattern, .. }) = search.pattern_matching_relative_path(
                    rel.as_bytes().as_bstr(),
                    Some(is_dir),
                    Case::Sensitive,
                ) {
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
        if let Ok(gi_path) = root.join(".gitignore")
            && gi_path.exists().unwrap_or(false)
                && let Ok(contents) = gi_path.read_to_string() {
                    search.add_patterns_buffer(
                        contents.as_bytes(),
                        PathBuf::from(gi_path.as_str()),
                        Some(root_path),
                    );
                }

        let mut files = Vec::new();
        let mut stack = vec![root.clone()];

        while let Some(path) = stack.pop() {
            let rel = path
                .as_str()
                .strip_prefix(root_str)
                .unwrap_or(path.as_str())
                .trim_start_matches('/');

            let meta = match path.metadata() {
                Ok(m) => m,
                Err(e) => {
                    logger.log(
                        LogLevel::Error,
                        &format!("metadata error on {}: {e}", path.as_str()),
                    );
                    continue;
                }
            };

            let is_dir = meta.file_type == VfsFileType::Directory;
            
            // Skip ignored paths. 
            // Note: For the root path itself (rel is empty), we typically don't skip unless explicitly ignored?
            // But ignored() logic should handle empty rel string if appropriate, or we can skip check for root.
            if !rel.is_empty() && ignored(&search, rel, is_dir) {
                continue;
            }

            if is_dir {
                // If this is a directory, we need to:
                // 1. Check for .gitignore and update search patterns
                // 2. Read children and add to stack

                if let Ok(gi) = path.join(".gitignore")
                    && gi.exists().unwrap_or(false)
                        && let Ok(contents) = gi.read_to_string() {
                            search.add_patterns_buffer(
                                contents.as_bytes(),
                                PathBuf::from(gi.as_str()),
                                Some(root_path),
                            );
                        }

                match path.read_dir() {
                    Ok(it) => {
                        for child in it {
                            stack.push(child);
                        }
                    }
                    Err(e) => {
                        logger.log(
                            LogLevel::Error,
                            &format!("failed to read dir {}: {e}", path.as_str()),
                        );
                    }
                }
            } else {
                files.push(path);
            }
        }
        Ok(files)
    }
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
        let walk = WalkBuilder::new(&root).build();
        let files = walk.collect_files(&logger).unwrap();
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
        let walk = WalkBuilder::new(&missing).build();
        let files = walk.collect_files(&logger).unwrap();
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
        let walk = WalkBuilder::new(&root).build();
        let files = walk.collect_files(&logger).unwrap();
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
        let walk = WalkBuilder::new(&root).build();
        let files = walk.collect_files(&logger).unwrap();
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
        let walk = WalkBuilder::new(&root).build();
        let files = walk.collect_files(&logger).unwrap();
        let paths: Vec<_> = files.iter().map(|p| p.as_str()).collect();
        assert!(paths.contains(&"/b.js"));
        assert!(paths.contains(&"/.gitignore"));
        assert!(paths.contains(&"/sub/.gitignore"));
        assert!(paths.contains(&"/sub/a.js"));
        assert!(paths.contains(&"/sub/c.js"));
        assert!(!paths.contains(&"/a.js"));
        assert!(!paths.contains(&"/sub/b.js"));
    }

    #[test]
    fn test_custom_ignore_patterns() {
        let fs = TestFS::new([("a.js", ""), ("ignored/b.js", "")]);
        let root = fs.root();
        let logger = crate::EmptyLogger;
        let walk = WalkBuilder::new(&root)
            .ignore_patterns(&["ignored/".to_string()])
            .build();
        let files = walk.collect_files(&logger).unwrap();
        let names: Vec<_> = files
            .iter()
            .map(|p| Path::new(p.as_str()).file_name().unwrap().to_str().unwrap())
            .collect();
        assert!(names.contains(&"a.js"));
        assert!(!names.contains(&"b.js"));
    }

    #[test]
    fn test_ignore_git_folder() {
        let fs = TestFS::new([(".git/config", ""), ("a.js", "")]);
        let root = fs.root();
        let logger = crate::EmptyLogger;
        let walk = WalkBuilder::new(&root).build();
        let files = walk.collect_files(&logger).unwrap();
        let paths: Vec<_> = files.iter().map(|p| p.as_str()).collect();
        assert!(paths.contains(&"/a.js"));
        assert!(!paths.iter().any(|p| p.contains("/.git/")));
    }
}
