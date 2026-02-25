use regex::Regex;
use std::path::Path;
use std::sync::OnceLock;
use vfs::VfsPath;

use dep_core::js_resolve::{
    JS_EXTENSIONS, is_node_builtin, resolve_alias_import, resolve_relative_import,
};
use dep_core::{Context, Edge, Parser};
use dep_core::{EdgeType, NodeKind};

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
        let src = path.read_to_string()?;
        let root_str = ctx.root.as_str().trim_end_matches('/');
        let rel = path
            .as_str()
            .strip_prefix(root_str)
            .unwrap_or(path.as_str())
            .trim_start_matches('/');
        let mut edges = Vec::new();
        static IMPORT_RE: OnceLock<Regex> = OnceLock::new();
        let re = IMPORT_RE.get_or_init(|| Regex::new(r#"^\s*import\s+(?:[^'\"]*?from\s+)?['\"]([^'\"]+)['\"]"#).expect("invalid regex"));
        let dir = path.parent();
        for cap in re.captures_iter(&src) {
            let spec = cap[1].to_string();
            let (target_str, to_type) = if spec.starts_with('.') {
                if let Some(target) = resolve_relative_import(&dir, &spec) {
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
                        None
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
                    None
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
                from_type: None,
                to_type,
            });
        }
        Ok(edges)
    }
}
