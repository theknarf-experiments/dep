use std::path::Path;
use vfs::VfsPath;

use crate::types::{Context, Parser};
use crate::{EdgeMetadata, NodeKind};

pub struct IndexParser;

impl Parser for IndexParser {
    fn name(&self) -> &'static str {
        "index"
    }

    fn can_parse(&self, path: &VfsPath) -> bool {
        if let Some(name) = Path::new(path.as_str())
            .file_name()
            .and_then(|s| s.to_str())
        {
            if let Some(ext) = Path::new(name).extension().and_then(|s| s.to_str()) {
                name.starts_with("index.") && crate::types::js::JS_EXTENSIONS.contains(&ext)
            } else {
                false
            }
        } else {
            false
        }
    }

    fn parse(&self, path: &VfsPath, ctx: &Context) -> anyhow::Result<()> {
        let root_str = ctx.root.as_str().trim_end_matches('/');
        let rel = path
            .as_str()
            .strip_prefix(root_str)
            .unwrap_or(path.as_str())
            .trim_start_matches('/');
        let parent_rel = Path::new(rel)
            .parent()
            .map(|p| p.to_str().unwrap())
            .unwrap_or("");
        let mut data = ctx.data.lock().unwrap();
        if let (Some(&folder_idx), Some(&file_idx)) = (
            data.nodes.get(&(parent_rel.to_string(), NodeKind::Folder)),
            data.nodes.get(&(rel.to_string(), NodeKind::File)),
        ) {
            data.graph
                .add_edge(folder_idx, file_idx, EdgeMetadata { same_as: true });
        }
        Ok(())
    }
}
