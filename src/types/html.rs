use regex::Regex;
use std::path::Path;
use vfs::VfsPath;

use crate::LogLevel;
use crate::types::js::{
    JS_EXTENSIONS, is_node_builtin, resolve_alias_import, resolve_relative_import,
};
use crate::types::{Context, Edge, Parser};
use crate::{Node, NodeKind, EdgeType};

pub struct HtmlParser;

impl Parser for HtmlParser {
    fn name(&self) -> &'static str {
        "html"
    }
    fn can_parse(&self, path: &VfsPath) -> bool {
        Path::new(path.as_str())
            .extension()
            .and_then(|s| s.to_str())
            == Some("html")
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
        let mut edges = Vec::new();
        let from_node = Node {
            name: rel.to_string(),
            kind: Some(NodeKind::File),
        };
        let re = Regex::new(r#"<script[^>]*src=[\"']([^\"']+)[\"'][^>]*>"#).unwrap();
        for cap in re.captures_iter(&src) {
            let spec = cap[1].to_string();
            let (target_str, kind) = if spec.starts_with('.') {
                if let Some(target) = resolve_relative_import(&path.parent(), &spec) {
                    let rel = target
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
                        None
                    } else {
                        Some(NodeKind::Asset)
                    };
                    (rel, kind)
                } else {
                    continue;
                }
            } else if let Some(target) = resolve_alias_import(ctx.aliases, &spec) {
                let rel = target
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
                    None
                } else {
                    Some(NodeKind::Asset)
                };
                (rel, kind)
            } else if is_node_builtin(&spec) {
                (spec.clone(), Some(NodeKind::Builtin))
            } else {
                (spec.clone(), Some(NodeKind::External))
            };
            let to_node = Node {
                name: target_str.clone(),
                kind,
            };
            edges.push(Edge {
                from: from_node.clone(),
                to: to_node,
                kind: EdgeType::Regular,
            });
        }
        Ok(edges)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build_dependency_graph;
    use crate::test_util::TestFS;

    #[test]
    fn test_html_parser_basic() {
        let fs = TestFS::new([
            ("index.html", "<script src=\"./app.js\"></script>"),
            ("app.js", ""),
        ]);
        let root = fs.root();
        let logger = crate::EmptyLogger;
        let walk = crate::WalkBuilder::new(&root).build();
        let graph = build_dependency_graph(&walk, None, &logger).unwrap();
        let html_idx = graph
            .node_indices()
            .find(|i| graph[*i].name == "index.html" && graph[*i].kind == Some(NodeKind::File))
            .unwrap();
        let js_idx = graph
            .node_indices()
            .find(|i| graph[*i].name == "app.js" && graph[*i].kind == Some(NodeKind::File))
            .unwrap();
        assert!(graph.find_edge(html_idx, js_idx).is_some());
    }

    #[test]
    fn test_html_parser_malformed() {
        let fs = TestFS::new([
            ("index.html", "<script src='broken.js'>"),
            ("broken.js", ""),
        ]);
        let root = fs.root();
        let logger = crate::EmptyLogger;
        let walk = crate::WalkBuilder::new(&root).build();
        let res = build_dependency_graph(&walk, None, &logger);
        assert!(res.is_ok());
    }
}
