use std::path::Path;
use vfs::VfsPath;

use crate::types::{Context, Parser};
use crate::{
    JS_EXTENSIONS, Node, NodeKind, ensure_folders, is_node_builtin, parse_file,
    resolve_alias_import, resolve_relative_import,
};

pub struct JsParser;

impl Parser for JsParser {
    fn name(&self) -> &'static str {
        "js"
    }
    fn can_parse(&self, path: &VfsPath) -> bool {
        let ext = Path::new(path.as_str())
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        JS_EXTENSIONS.contains(&ext)
    }

    fn parse(&self, path: &VfsPath, ctx: &Context) -> anyhow::Result<()> {
        let root_str = ctx.root.as_str().trim_end_matches('/');
        let rel = path
            .as_str()
            .strip_prefix(root_str)
            .unwrap_or(path.as_str())
            .trim_start_matches('/');
        let imports = parse_file(path, ctx.color).unwrap_or_default();
        let from_idx;
        {
            let mut data = ctx.data.lock().unwrap();
            let parent_idx = ensure_folders(rel, &mut data, ctx.root_idx);
            let key = (rel.to_string(), NodeKind::File);
            from_idx = if let Some(&idx) = data.nodes.get(&key) {
                idx
            } else {
                let idx = data.graph.add_node(Node {
                    name: rel.to_string(),
                    kind: NodeKind::File,
                });
                data.nodes.insert(key, idx);
                idx
            };
            if data.graph.find_edge(parent_idx, from_idx).is_none() {
                data.graph.add_edge(parent_idx, from_idx, ());
            }
        }
        let dir = path.parent();
        for i in imports {
            let (target_str, kind) = if i.starts_with('.') {
                if let Some(target) = resolve_relative_import(&dir, &i) {
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
            } else if let Some(target) = resolve_alias_import(ctx.aliases, &i) {
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
            } else if is_node_builtin(&i) {
                (i.clone(), NodeKind::Builtin)
            } else {
                (i.clone(), NodeKind::External)
            };
            let mut data = ctx.data.lock().unwrap();
            let key = (target_str.clone(), kind.clone());
            let to_idx = if let Some(&idx) = data.nodes.get(&key) {
                idx
            } else {
                let idx = data.graph.add_node(Node {
                    name: target_str.clone(),
                    kind: kind.clone(),
                });
                data.nodes.insert(key, idx);
                idx
            };
            data.graph.add_edge(from_idx, to_idx, ());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::TestFS;

    #[test]
    fn test_js_parser_basic() {
        let fs = TestFS::new([("a.js", "import './b.js';"), ("b.js", "")]);
        let root = fs.root();
        let graph = crate::build_dependency_graph(&root, Default::default()).unwrap();
        assert!(graph.node_indices().any(|i| graph[i].name == "a.js"));
    }

    #[test]
    fn test_js_parser_malformed() {
        let fs = TestFS::new([("a.js", "import ???")]);
        let root = fs.root();
        let res = crate::build_dependency_graph(&root, Default::default());
        assert!(res.is_ok());
    }
}
