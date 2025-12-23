use regex::Regex;
use std::path::Path;
use vfs::{VfsFileType, VfsPath};

use crate::types::js::JS_EXTENSIONS;
use crate::types::{Context, Edge, Parser};
use crate::{EdgeType, LogLevel, NodeKind};

fn expand_glob(base: &VfsPath, pat: &str) -> anyhow::Result<Vec<VfsPath>> {
    let pattern = match pat.strip_prefix("./") {
        Some(p) => glob::Pattern::new(p)?,
        None => glob::Pattern::new(pat)?,
    };
    let base_str = base.as_str().trim_end_matches('/');
    let mut matches = Vec::new();
    let walk = match base.walk_dir() {
        Ok(w) => w,
        Err(e) => {
            return Err(anyhow::anyhow!(format!("walk error: {e}")));
        }
    };
    for entry in walk {
        let path = match entry {
            Ok(p) => p,
            Err(_) => continue,
        };
        let meta = match path.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.file_type != VfsFileType::File {
            continue;
        }
        let rel = path
            .as_str()
            .strip_prefix(base_str)
            .unwrap_or(path.as_str())
            .trim_start_matches('/');
        if pattern.matches(rel) {
            matches.push(path);
        }
    }
    Ok(matches)
}

pub struct ViteParser;

impl Parser for ViteParser {
    fn name(&self) -> &'static str {
        "vite_glob"
    }

    fn can_parse(&self, path: &VfsPath) -> bool {
        let ext = Path::new(path.as_str())
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        JS_EXTENSIONS.contains(&ext)
    }

    fn parse(&self, path: &VfsPath, ctx: &Context) -> anyhow::Result<Vec<Edge>> {
        let src = match path.read_to_string() {
            Ok(s) => s,
            Err(e) => {
                ctx.logger.log(
                    LogLevel::Error,
                    &format!("failed to read {}: {e}", path.as_str()),
                );
                return Ok(Vec::new());
            }
        };
        let re = Regex::new(r#"import\.meta\.glob(?:Eager)?\(\s*['"]([^'"]+)['"]"#).unwrap();
        let dir = path.parent();
        let root_str = ctx.root.as_str().trim_end_matches('/');
        let rel = path
            .as_str()
            .strip_prefix(root_str)
            .unwrap_or(path.as_str())
            .trim_start_matches('/');
        let mut edges = Vec::new();
        for cap in re.captures_iter(&src) {
            let pattern = cap[1].to_string();
            let Ok(files) = expand_glob(&dir, &pattern) else {
                continue;
            };
            for f in files {
                let rel_path = f
                    .as_str()
                    .strip_prefix(root_str)
                    .unwrap_or(f.as_str())
                    .trim_start_matches('/');
                let ext = Path::new(f.as_str())
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("");
                let to_type = if JS_EXTENSIONS.contains(&ext) {
                    None // File is default
                } else {
                    Some(NodeKind::Asset)
                };
                edges.push(Edge {
                    from: rel.to_string(),
                    to: rel_path.to_string(),
                    kind: EdgeType::Regular,
                    from_type: None, // File is default
                    to_type,
                });
            }
        }
        Ok(edges)
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::TestFS;

    #[test]
    fn test_vite_glob_basic() {
        let fs = TestFS::new([
            (
                "index.ts",
                "const modules = import.meta.glob('./foo/*.jsx', { eager: true }) as any;",
            ),
            ("foo/a.jsx", ""),
            ("foo/b.jsx", ""),
        ]);
        let root = fs.root();
        let logger = crate::EmptyLogger;
        let walk = crate::WalkBuilder::new(&root).build();
        let graph = crate::build_dependency_graph(&walk, None, &logger).unwrap();
        let idx_index = graph
            .node_indices()
            .find(|i| graph[*i].name == "index.ts")
            .unwrap();
        let idx_a = graph
            .node_indices()
            .find(|i| graph[*i].name == "foo/a.jsx")
            .unwrap();
        let idx_b = graph
            .node_indices()
            .find(|i| graph[*i].name == "foo/b.jsx")
            .unwrap();
        assert!(graph.find_edge(idx_index, idx_a).is_some());
        assert!(graph.find_edge(idx_index, idx_b).is_some());
    }

    #[test]
    fn test_vite_glob_asset() {
        let fs = TestFS::new([
            (
                "index.js",
                "const imgs = import.meta.glob('./assets/*.png', { eager: true }) as any;",
            ),
            ("assets/logo.png", ""),
        ]);
        let root = fs.root();
        let logger = crate::EmptyLogger;
        let walk = crate::WalkBuilder::new(&root).build();
        let graph = crate::build_dependency_graph(&walk, None, &logger).unwrap();
        let idx_index = graph
            .node_indices()
            .find(|i| graph[*i].name == "index.js")
            .unwrap();
        let idx_logo = graph
            .node_indices()
            .find(|i| graph[*i].name == "assets/logo.png")
            .unwrap();
        assert!(graph.find_edge(idx_index, idx_logo).is_some());
    }
}
