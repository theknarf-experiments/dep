use regex::Regex;
use std::path::Path;
use vfs::VfsPath;

use crate::types::{Context, Parser};
use crate::{
    JS_EXTENSIONS, Node, NodeKind, ensure_folders, is_node_builtin, resolve_alias_import,
    resolve_relative_import,
};

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

    fn parse(&self, path: &VfsPath, ctx: &Context) -> anyhow::Result<()> {
        let src = match path.read_to_string() {
            Ok(s) => s,
            Err(e) => {
                crate::log_error(ctx.color, &format!("failed to read {}: {e}", path.as_str()));
                return Ok(());
            }
        };
        let root_str = ctx.root.as_str().trim_end_matches('/');
        let rel = path
            .as_str()
            .strip_prefix(root_str)
            .unwrap_or(path.as_str())
            .trim_start_matches('/');
        {
            let mut data = ctx.data.lock().unwrap();
            let parent_idx = ensure_folders(rel, &mut data, ctx.root_idx);
            let key = (rel.to_string(), NodeKind::File);
            let file_idx = if let Some(&i) = data.nodes.get(&key) {
                i
            } else {
                let i = data.graph.add_node(Node {
                    name: rel.to_string(),
                    kind: NodeKind::File,
                });
                data.nodes.insert(key, i);
                i
            };
            if data.graph.find_edge(parent_idx, file_idx).is_none() {
                data.graph.add_edge(parent_idx, file_idx, ());
            }
        }
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
                        NodeKind::File
                    } else {
                        NodeKind::Asset
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
                    NodeKind::File
                } else {
                    NodeKind::Asset
                };
                (rel, kind)
            } else if is_node_builtin(&spec) {
                (spec.clone(), NodeKind::Builtin)
            } else {
                (spec.clone(), NodeKind::External)
            };
            let mut data = ctx.data.lock().unwrap();
            let from_idx = data.nodes[&(rel.to_string(), NodeKind::File)];
            let key = (target_str.clone(), kind.clone());
            let to_idx = if let Some(&i) = data.nodes.get(&key) {
                i
            } else {
                let i = data.graph.add_node(Node {
                    name: target_str.clone(),
                    kind: kind.clone(),
                });
                data.nodes.insert(key, i);
                i
            };
            data.graph.add_edge(from_idx, to_idx, ());
        }
        Ok(())
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
        let graph = build_dependency_graph(&root, Default::default()).unwrap();
        let html_idx = graph
            .node_indices()
            .find(|i| graph[*i].name == "index.html" && graph[*i].kind == NodeKind::File)
            .unwrap();
        let js_idx = graph
            .node_indices()
            .find(|i| graph[*i].name == "app.js" && graph[*i].kind == NodeKind::File)
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
        let res = build_dependency_graph(&root, Default::default());
        assert!(res.is_ok());
    }
}
