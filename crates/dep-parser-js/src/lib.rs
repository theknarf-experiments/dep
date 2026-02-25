use regex::Regex;
use std::path::Path;
use std::sync::OnceLock;
use vfs::VfsPath;

use dep_core::js_resolve::{
    JS_EXTENSIONS, is_node_builtin, resolve_alias_import, resolve_relative_import,
};
use dep_core::{Context, Edge, Parser, Logger};
use dep_core::{NodeKind, EdgeType};
use swc_common::{FileName, SourceMap, sync::Lrc};
use swc_ecma_ast::{Module, ModuleDecl, ModuleItem};
use swc_ecma_parser::{EsConfig, Parser as SwcParser, StringInput, Syntax, TsConfig};

pub fn parse_module(src: &str, ext: &str, file: FileName) -> anyhow::Result<Module> {
    let cm: Lrc<SourceMap> = Default::default();
    let fm = cm.new_source_file(file, src.into());
    let syntax = match ext {
        "ts" | "tsx" | "mts" | "cts" => Syntax::Typescript(TsConfig {
            tsx: true,
            ..Default::default()
        }),
        _ => Syntax::Es(EsConfig {
            jsx: true,
            ..Default::default()
        }),
    };
    let mut parser = SwcParser::new(syntax, StringInput::from(&*fm), None);
    parser
        .parse_module()
        .map_err(|e| anyhow::anyhow!(format!("{:?}", e)))
}

/// Parse a JS/TS file and return the list of relative imports.
pub fn parse_file(path: &VfsPath, _logger: &dyn Logger) -> anyhow::Result<Vec<String>> {
    let src = path.read_to_string()?;
    let ext = Path::new(path.as_str())
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let module = parse_module(&src, ext, FileName::Custom(path.as_str().into()))?;
    let mut imports = collect_imports(&module);

    static REQUIRE_RE: OnceLock<Regex> = OnceLock::new();
    let re = REQUIRE_RE.get_or_init(|| Regex::new(r#"require\(\s*['\"]([^'\"]+)['\"]\s*\)"#).expect("invalid regex"));

    for cap in re.captures_iter(&src) {
        imports.push(cap[1].to_string());
    }
    Ok(imports)
}

/// Collect import specifiers from a parsed module.
pub fn collect_imports(module: &Module) -> Vec<String> {
    let mut imports = Vec::new();
    for item in &module.body {
        if let ModuleItem::ModuleDecl(decl) = item {
            match decl {
                ModuleDecl::Import(import) => {
                    imports.push(import.src.value.to_string());
                }
                ModuleDecl::ExportAll(export) => {
                    imports.push(export.src.value.to_string());
                }
                ModuleDecl::ExportNamed(named) => {
                    if let Some(src) = &named.src {
                        imports.push(src.value.to_string());
                    }
                }
                _ => {}
            }
        }
    }
    imports
}

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

    fn parse(&self, path: &VfsPath, ctx: &Context) -> anyhow::Result<Vec<Edge>> {
        let root_str = ctx.root.as_str().trim_end_matches('/');
        let rel = path
            .as_str()
            .strip_prefix(root_str)
            .unwrap_or(path.as_str())
            .trim_start_matches('/');
        let imports = parse_file(path, ctx.logger)?;
        let mut edges = Vec::new();
        let dir = path.parent();
        for i in imports {
            let (target_str, to_type) = if i.starts_with('.') {
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
                    let to_type = if JS_EXTENSIONS.contains(&ext) {
                        None
                    } else {
                        Some(NodeKind::Asset)
                    };
                    (rel, to_type)
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
                let to_type = if JS_EXTENSIONS.contains(&ext) {
                    None
                } else {
                    Some(NodeKind::Asset)
                };
                (rel, to_type)
            } else if is_node_builtin(&i) {
                (i.clone(), Some(NodeKind::Builtin))
            } else {
                (i.clone(), Some(NodeKind::External))
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

#[cfg(test)]
mod tests {
    use super::*;
    use dep_core::test_util::TestFS;
    use swc_common::FileName;

    #[test]
    fn test_parse_file_missing() {
        let fs = TestFS::new([("a.js", "")]);
        let root = fs.root();
        let missing = root.join("missing.js").unwrap();
        let logger = dep_core::EmptyLogger;
        let res = parse_file(&missing, &logger);
        assert!(res.is_err());
    }

    #[test]
    fn test_collect_imports_from_string() {
        let src =
            "import foo from './foo';\nexport * from './bar';\nexport { baz } from './baz.js';";
        let module = parse_module(src, "js", FileName::Custom("test.js".into())).unwrap();
        let imports = collect_imports(&module);
        assert_eq!(
            imports,
            vec!["./foo", "./bar", "./baz.js"]
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        );
    }
}
