use std::path::Path;
use vfs::VfsPath;

use crate::types::js::JS_EXTENSIONS;
use crate::types::{Context, Parser};
use crate::{Edge, Node, NodeKind, ensure_folders};

pub struct IndexFileParser;

impl Parser for IndexFileParser {
    fn name(&self) -> &'static str {
        "index_file"
    }

    fn can_parse(&self, path: &VfsPath) -> bool {
        match path.filename() {
            name if name.starts_with("index.") => {
                let ext = Path::new(path.as_str())
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("");
                JS_EXTENSIONS.contains(&ext)
            }
            _ => false,
        }
    }

    fn parse(&self, path: &VfsPath, ctx: &Context) -> anyhow::Result<()> {
        let root_str = ctx.root.as_str().trim_end_matches('/');
        let rel = path
            .as_str()
            .strip_prefix(root_str)
            .unwrap_or(path.as_str())
            .trim_start_matches('/');
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
        if let Some(eidx) = data.graph.find_edge(parent_idx, file_idx) {
            data.graph
                .edge_weight_mut(eidx)
                .unwrap()
                .metadata
                .insert("sameAs".to_string(), "true".to_string());
        } else {
            let mut edge = Edge::default();
            edge.metadata
                .insert("sameAs".to_string(), "true".to_string());
            data.graph.add_edge(parent_idx, file_idx, edge);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::TestFS;

    #[test]
    fn test_index_sameas_metadata() {
        let fs = TestFS::new([("dir/index.js", ""), ("dir/other.js", "")]);
        let root = fs.root();
        let graph = crate::build_dependency_graph(&root, Default::default()).unwrap();
        let folder_idx = graph
            .node_indices()
            .find(|i| graph[*i].name == "dir" && graph[*i].kind == NodeKind::Folder)
            .unwrap();
        let index_idx = graph
            .node_indices()
            .find(|i| graph[*i].name == "dir/index.js" && graph[*i].kind == NodeKind::File)
            .unwrap();
        let edge_idx = graph.find_edge(folder_idx, index_idx).unwrap();
        let edge = graph.edge_weight(edge_idx).unwrap();
        assert!(edge.metadata.contains_key("sameAs"));
    }
}
