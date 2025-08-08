use std::path::Path;
use vfs::VfsPath;

use crate::types::util::JS_EXTENSIONS;
use crate::types::{Context, Edge, Parser};
use crate::{EdgeType, Node, NodeKind};

pub struct IndexParser;

impl Parser for IndexParser {
    fn name(&self) -> &'static str {
        "index"
    }

    fn can_parse(&self, path: &VfsPath) -> bool {
        let name = path.filename();
        if let Some(ext) = Path::new(path.as_str())
            .extension()
            .and_then(|s| s.to_str())
        {
            name.starts_with("index.") && JS_EXTENSIONS.contains(&ext)
        } else {
            false
        }
    }

    fn parse(&self, path: &VfsPath, ctx: &Context) -> anyhow::Result<Vec<Edge>> {
        let root_str = ctx.root.as_str().trim_end_matches('/');
        let rel = path
            .as_str()
            .strip_prefix(root_str)
            .unwrap_or(path.as_str())
            .trim_start_matches('/');
        let parent = path.parent();
        let parent_rel = parent
            .as_str()
            .strip_prefix(root_str)
            .unwrap_or(parent.as_str())
            .trim_start_matches('/');
        Ok(vec![Edge {
            from: Node {
                name: parent_rel.to_string(),
                kind: NodeKind::Folder,
            },
            to: Node {
                name: rel.to_string(),
                kind: NodeKind::File,
            },
            kind: EdgeType::SameAs,
        }])
    }
}
