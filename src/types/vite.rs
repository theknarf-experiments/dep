use regex::Regex;
use std::path::Path;
use vfs::{VfsFileType, VfsPath};

use crate::LogLevel;
use crate::types::js::JS_EXTENSIONS;
use crate::types::{Context, Edge, Parser};
use crate::{EdgeType, Node, NodeKind};

fn find_glob_matches(dir: &VfsPath, pattern: &str) -> anyhow::Result<Vec<VfsPath>> {
    let pat_str = pattern.trim_start_matches("./");
    let pat = match glob::Pattern::new(pat_str) {
        Ok(p) => p,
        Err(_) => return Ok(Vec::new()),
    };
    let base = dir.as_str();
    let mut list = Vec::new();
    for entry in dir.walk_dir()? {
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
            .strip_prefix(base)
            .unwrap_or(path.as_str())
            .trim_start_matches('/');
        if pat.matches(rel) {
            list.push(path);
        }
    }
    Ok(list)
}

pub struct ViteParser;

impl Parser for ViteParser {
    fn name(&self) -> &'static str {
        "vite"
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
        let root_str = ctx.root.as_str().trim_end_matches('/');
        let rel = path
            .as_str()
            .strip_prefix(root_str)
            .unwrap_or(path.as_str())
            .trim_start_matches('/');
        let from_node = Node {
            name: rel.to_string(),
            kind: NodeKind::File,
        };
        let dir = path.parent();
        let call_re = Regex::new(r#"import\.meta\.glob(?:Eager)?\(([^)]*)\)"#).unwrap();
        let str_re = Regex::new(r#"['\"]([^'\"]+)['\"]"#).unwrap();
        let mut edges = Vec::new();
        for cap in call_re.captures_iter(&src) {
            let args = &cap[1];
            for s in str_re.captures_iter(args) {
                let pattern = &s[1];
                let targets = find_glob_matches(&dir, pattern)?;
                for target in targets {
                    let target_rel = target
                        .as_str()
                        .strip_prefix(root_str)
                        .unwrap_or(target.as_str())
                        .trim_start_matches('/')
                        .to_string();
                    let ext = Path::new(target.as_str())
                        .extension()
                        .and_then(|s| s.to_str())
                        .unwrap_or("");
                    let kind = if JS_EXTENSIONS.contains(&ext) {
                        NodeKind::File
                    } else {
                        NodeKind::Asset
                    };
                    edges.push(Edge {
                        from: from_node.clone(),
                        to: Node {
                            name: target_rel,
                            kind,
                        },
                        kind: EdgeType::Regular,
                    });
                }
            }
        }
        Ok(edges)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::TestFS;

    #[test]
    fn test_vite_glob_basic() {
        let fs = TestFS::new([
            (
                "src/index.js",
                "const mods = import.meta.glob('./**/*.jsx');",
            ),
            ("src/app.jsx", ""),
            ("src/sub/comp.jsx", ""),
        ]);
        let root = fs.root();
        let logger = crate::EmptyLogger;
        let walk = crate::WalkBuilder::new(&root).build();
        let graph = crate::build_dependency_graph(&walk, None, &logger).unwrap();
        let idx_js = graph
            .node_indices()
            .find(|i| graph[*i].name == "src/index.js" && graph[*i].kind == NodeKind::File)
            .unwrap();
        let idx_app = graph
            .node_indices()
            .find(|i| graph[*i].name == "src/app.jsx" && graph[*i].kind == NodeKind::File)
            .unwrap();
        let idx_comp = graph
            .node_indices()
            .find(|i| graph[*i].name == "src/sub/comp.jsx" && graph[*i].kind == NodeKind::File)
            .unwrap();
        assert!(graph.find_edge(idx_js, idx_app).is_some());
        assert!(graph.find_edge(idx_js, idx_comp).is_some());
    }
}
