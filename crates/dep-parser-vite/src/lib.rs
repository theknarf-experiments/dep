use regex::Regex;
use std::path::Path;
use std::sync::OnceLock;
use vfs::{VfsFileType, VfsPath};

use dep_core::js_resolve::JS_EXTENSIONS;
use dep_core::{Context, Edge, Parser};
use dep_core::{EdgeType, NodeKind};

fn expand_glob(base: &VfsPath, pat: &str) -> anyhow::Result<Vec<VfsPath>> {
    let pattern = match pat.strip_prefix("./") {
        Some(p) => glob::Pattern::new(p)?,
        None => glob::Pattern::new(pat)?,
    };
    let base_str = base.as_str().trim_end_matches('/');
    let mut matches = Vec::new();
    let walk = match base.walk_dir() {
        Ok(w) => w,
        Err(e) => {
            return Err(anyhow::anyhow!(format!("walk error: {e}")));
        }
    };
    for entry in walk {
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
            .strip_prefix(base_str)
            .unwrap_or(path.as_str())
            .trim_start_matches('/');
        if pattern.matches(rel) {
            matches.push(path);
        }
    }
    Ok(matches)
}

pub struct ViteParser;

impl Parser for ViteParser {
    fn name(&self) -> &'static str {
        "vite_glob"
    }

    fn can_parse(&self, path: &VfsPath) -> bool {
        let ext = Path::new(path.as_str())
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        JS_EXTENSIONS.contains(&ext)
    }

    fn parse(&self, path: &VfsPath, ctx: &Context) -> anyhow::Result<Vec<Edge>> {
        let src = path.read_to_string()?;
        static GLOB_RE: OnceLock<Regex> = OnceLock::new();
        let re = GLOB_RE.get_or_init(|| Regex::new(r#"import\.meta\.glob(?:Eager)?\(\s*['"]([^'"]+)['"]"#).expect("invalid regex"));
        let dir = path.parent();
        let root_str = ctx.root.as_str().trim_end_matches('/');
        let rel = path
            .as_str()
            .strip_prefix(root_str)
            .unwrap_or(path.as_str())
            .trim_start_matches('/');
        let mut edges = Vec::new();
        for cap in re.captures_iter(&src) {
            let pattern = cap[1].to_string();
            let Ok(files) = expand_glob(&dir, &pattern) else {
                continue;
            };
            for f in files {
                let rel_path = f
                    .as_str()
                    .strip_prefix(root_str)
                    .unwrap_or(f.as_str())
                    .trim_start_matches('/');
                let ext = Path::new(f.as_str())
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("");
                let to_type = if JS_EXTENSIONS.contains(&ext) {
                    None
                } else {
                    Some(NodeKind::Asset)
                };
                edges.push(Edge {
                    from: rel.to_string(),
                    to: rel_path.to_string(),
                    kind: EdgeType::Regular,
                    from_type: None,
                    to_type,
                });
            }
        }
        Ok(edges)
    }
}
