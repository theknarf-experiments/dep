use regex::Regex;
use std::path::Path;
use vfs::VfsPath;

use crate::types::js::{
    is_node_builtin, resolve_alias_import, resolve_relative_import,
};
use crate::types::{Context, Edge, Parser};
use crate::{EdgeType, LogLevel, Node, NodeKind};

pub struct MdxParser;

impl Parser for MdxParser {
    fn name(&self) -> &'static str {
        "mdx"
    }

    fn can_parse(&self, path: &VfsPath) -> bool {
        Path::new(path.as_str())
            .extension()
            .and_then(|s| s.to_str())
            == Some("mdx")
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
            kind: Some(NodeKind::File),
        };
        let mut edges = Vec::new();
        let re = Regex::new(r#"^\s*import\s+(?:[^'\"]*?from\s+)?['\"]([^'\"]+)['\"]"#).unwrap();
        let dir = path.parent();
        for cap in re.captures_iter(&src) {
            let spec = cap[1].to_string();
            let (target_str, kind) = if spec.starts_with('.') {
                if let Some(target) = resolve_relative_import(&dir, &spec) {
                    let rel = target
                        .as_str()
                        .strip_prefix(root_str)
                        .unwrap_or(target.as_str())
                        .trim_start_matches('/')
                        .to_string();
                    (rel, None)
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
                (rel, None)
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
    use crate::test_util::TestFS;

    #[test]
    fn test_mdx_parser_basic() {
        let fs = TestFS::new([
            ("index.mdx", "import Foo from './foo.js'\n\n# Hello"),
            ("foo.js", ""),
        ]);
        let root = fs.root();
        let logger = crate::EmptyLogger;
        let walk = crate::WalkBuilder::new(&root).build();
        let graph = crate::build_dependency_graph(&walk, None, &logger).unwrap();
        let mdx_idx = graph
            .node_indices()
            .find(|i| graph[*i].name == "index.mdx" && graph[*i].kind == Some(NodeKind::File))
            .unwrap();
        let foo_idx = graph
            .node_indices()
            .find(|i| graph[*i].name == "foo.js" && graph[*i].kind == Some(NodeKind::File))
            .unwrap();
        assert!(graph.find_edge(mdx_idx, foo_idx).is_some());
    }
}
