use regex::Regex;
use std::path::Path;
use std::sync::OnceLock;
use vfs::VfsPath;

use crate::types::js::{
    JS_EXTENSIONS, is_node_builtin, resolve_alias_import, resolve_relative_import,
};
use crate::types::{Context, Edge, Parser};
use crate::{NodeKind, EdgeType};

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
        let src = path.read_to_string()?;
        let root_str = ctx.root.as_str().trim_end_matches('/');
        let rel = path
            .as_str()
            .strip_prefix(root_str)
            .unwrap_or(path.as_str())
            .trim_start_matches('/');
        let mut edges = Vec::new();
        static SCRIPT_RE: OnceLock<Regex> = OnceLock::new();
        let re = SCRIPT_RE.get_or_init(|| Regex::new(r#"<script[^>]*src=[\"']([^\"']+)[\"'][^>]*>"#).expect("invalid regex"));
        for cap in re.captures_iter(&src) {
            let spec = cap[1].to_string();
            let (target_str, to_type) = if spec.starts_with('.') {
                if let Some(target) = resolve_relative_import(&path.parent(), &spec) {
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
                    let to_type = if JS_EXTENSIONS.contains(&ext) {
                        None // File is default
                    } else {
                        Some(NodeKind::Asset)
                    };
                    (target_rel, to_type)
                } else {
                    continue;
                }
            } else if let Some(target) = resolve_alias_import(ctx.aliases, &spec) {
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
                let to_type = if JS_EXTENSIONS.contains(&ext) {
                    None // File is default
                } else {
                    Some(NodeKind::Asset)
                };
                (target_rel, to_type)
            } else if is_node_builtin(&spec) {
                (spec.clone(), Some(NodeKind::Builtin))
            } else {
                (spec.clone(), Some(NodeKind::External))
            };
            edges.push(Edge {
                from: rel.to_string(),
                to: target_str,
                kind: EdgeType::Regular,
                from_type: None, // File is default
                to_type,
            });
        }
        Ok(edges)
    }
}

#[cfg(test)]
mod tests {
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
            .find(|i| graph[*i].name == "index.html")
            .unwrap();
        let js_idx = graph
            .node_indices()
            .find(|i| graph[*i].name == "app.js")
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
