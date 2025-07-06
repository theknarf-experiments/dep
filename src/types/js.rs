use regex::Regex;
use std::path::Path;
use vfs::VfsPath;

use crate::types::{Context, Edge, Parser};
use crate::{LogLevel, Logger};
use crate::{Node, NodeKind, EdgeType};
use swc_common::{FileName, SourceMap, sync::Lrc};
use swc_ecma_ast::{Module, ModuleDecl, ModuleItem};
use swc_ecma_parser::{EsConfig, Parser as SwcParser, StringInput, Syntax, TsConfig};

pub(crate) const JS_EXTENSIONS: &[&str] = &["js", "jsx", "ts", "tsx", "mjs", "cjs", "mts", "cts"];

pub(crate) fn is_node_builtin(name: &str) -> bool {
    let n = name.strip_prefix("node:").unwrap_or(name);
    matches!(
        n,
        "assert"
            | "buffer"
            | "child_process"
            | "cluster"
            | "console"
            | "constants"
            | "crypto"
            | "dgram"
            | "dns"
            | "domain"
            | "events"
            | "fs"
            | "http"
            | "https"
            | "module"
            | "net"
            | "os"
            | "path"
            | "process"
            | "punycode"
            | "querystring"
            | "readline"
            | "repl"
            | "stream"
            | "string_decoder"
            | "timers"
            | "tls"
            | "tty"
            | "url"
            | "util"
            | "v8"
            | "vm"
            | "zlib"
    )
}

pub(crate) fn parse_module(src: &str, ext: &str, file: FileName) -> anyhow::Result<Module> {
    let cm: Lrc<SourceMap> = Default::default();
    let fm = cm.new_source_file(file, src.into());
    let syntax = match ext {
        "ts" | "tsx" | "mts" | "cts" => Syntax::Typescript(TsConfig::default()),
        _ => Syntax::Es(EsConfig::default()),
    };
    let mut parser = SwcParser::new(syntax, StringInput::from(&*fm), None);
    parser
        .parse_module()
        .map_err(|e| anyhow::anyhow!(format!("{:?}", e)))
}

/// Parse a JS/TS file and return the list of relative imports.
pub(crate) fn parse_file(path: &VfsPath, logger: &dyn Logger) -> anyhow::Result<Vec<String>> {
    let src = match path.read_to_string() {
        Ok(s) => s,
        Err(e) => {
            logger.log(
                LogLevel::Error,
                &format!("failed to read {}: {e}", path.as_str()),
            );
            return Ok(Vec::new());
        }
    };
    let ext = Path::new(path.as_str())
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let module = parse_module(&src, ext, FileName::Custom(path.as_str().into()))?;
    let mut imports = collect_imports(&module);
    let re = Regex::new(r#"require\(\s*['\"]([^'\"]+)['\"]\s*\)"#).unwrap();
    for cap in re.captures_iter(&src) {
        imports.push(cap[1].to_string());
    }
    Ok(imports)
}

/// Collect import specifiers from a parsed module.
pub(crate) fn collect_imports(module: &Module) -> Vec<String> {
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

pub(crate) fn resolve_relative_import(dir: &VfsPath, spec: &str) -> Option<VfsPath> {
    if let Ok(base) = dir.join(spec) {
        if base.exists().ok()? {
            return Some(base);
        }
        let p = Path::new(spec);
        if p.extension().is_none() {
            for ext in JS_EXTENSIONS {
                if let Ok(candidate) = dir.join(format!("{spec}.{}", ext)) {
                    if candidate.exists().ok()? {
                        return Some(candidate);
                    }
                }
            }
            for ext in JS_EXTENSIONS {
                if let Ok(candidate) = base.join(format!("index.{}", ext)) {
                    if candidate.exists().ok()? {
                        return Some(candidate);
                    }
                }
            }
        }
    }
    None
}

pub(crate) fn resolve_alias_import(aliases: &[(String, VfsPath)], spec: &str) -> Option<VfsPath> {
    for (alias, base) in aliases {
        if spec == alias || spec.starts_with(&format!("{}/", alias)) {
            let rest = if spec == alias {
                ""
            } else {
                &spec[alias.len() + 1..]
            };
            if let Ok(candidate_base) = base.join(rest) {
                if candidate_base.exists().ok()? {
                    return Some(candidate_base);
                }
                let p = Path::new(rest);
                if p.extension().is_none() {
                    for ext in JS_EXTENSIONS {
                        if let Ok(candidate) = base.join(format!("{rest}.{}", ext)) {
                            if candidate.exists().ok()? {
                                return Some(candidate);
                            }
                        }
                    }
                    for ext in JS_EXTENSIONS {
                        if let Ok(candidate) = candidate_base.join(format!("index.{}", ext)) {
                            if candidate.exists().ok()? {
                                return Some(candidate);
                            }
                        }
                    }
                }
            }
        }
    }
    None
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
        let imports = parse_file(path, ctx.logger).unwrap_or_default();
        let mut edges = Vec::new();
        let from_node = Node {
            name: rel.to_string(),
            kind: NodeKind::File,
        };
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
            let to_node = Node {
                name: target_str.clone(),
                kind: kind.clone(),
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
    use proptest::prelude::*;
    use swc_common::FileName;

    #[test]
    fn test_js_parser_basic() {
        let fs = TestFS::new([("a.js", "import './b.js';"), ("b.js", "")]);
        let root = fs.root();
        let logger = crate::EmptyLogger;
        let walk = crate::WalkBuilder::new(&root).build();
        let graph = crate::build_dependency_graph(&walk, None, &logger).unwrap();
        assert!(graph.node_indices().any(|i| graph[i].name == "a.js"));
    }

    #[test]
    fn test_js_parser_malformed() {
        let fs = TestFS::new([("a.js", "import ???")]);
        let root = fs.root();
        let logger = crate::EmptyLogger;
        let walk = crate::WalkBuilder::new(&root).build();
        let res = crate::build_dependency_graph(&walk, None, &logger);
        assert!(res.is_ok());
    }

    #[test]
    fn test_parse_file_missing() {
        let fs = TestFS::new([("a.js", "")]);
        let root = fs.root();
        let missing = root.join("missing.js").unwrap();
        let logger = crate::EmptyLogger;
        let imports = parse_file(&missing, &logger).unwrap();
        assert!(imports.is_empty());
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

    #[test]
    fn test_mixed_extension_imports() {
        let fs = TestFS::new([
            ("a.ts", "import './b';\nimport './c.js';"),
            ("b.ts", ""),
            ("c.js", ""),
        ]);
        let root = fs.root();
        let logger = crate::EmptyLogger;
        let walk = crate::WalkBuilder::new(&root).build();
        let graph = crate::build_dependency_graph(&walk, None, &logger).unwrap();
        let a_idx = graph
            .node_indices()
            .find(|i| graph[*i].name == "a.ts" && graph[*i].kind == NodeKind::File)
            .unwrap();
        let b_idx = graph
            .node_indices()
            .find(|i| graph[*i].name == "b.ts" && graph[*i].kind == NodeKind::File)
            .unwrap();
        let c_idx = graph
            .node_indices()
            .find(|i| graph[*i].name == "c.js" && graph[*i].kind == NodeKind::File)
            .unwrap();
        assert!(graph.find_edge(a_idx, b_idx).is_some());
        assert!(graph.find_edge(a_idx, c_idx).is_some());
    }

    #[test]
    fn test_asset_node_kind() {
        let fs = TestFS::new([("index.js", "import './logo.svg';"), ("logo.svg", "")]);
        let root = fs.root();
        let logger = crate::EmptyLogger;
        let walk = crate::WalkBuilder::new(&root).build();
        let graph = crate::build_dependency_graph(&walk, None, &logger).unwrap();
        let js_idx = graph
            .node_indices()
            .find(|i| graph[*i].name == "index.js" && graph[*i].kind == NodeKind::File)
            .unwrap();
        let asset_idx = graph
            .node_indices()
            .find(|i| graph[*i].name == "logo.svg" && graph[*i].kind == NodeKind::Asset)
            .unwrap();
        assert!(graph.find_edge(js_idx, asset_idx).is_some());
    }

    #[test]
    fn test_require_and_module_exports() {
        let fs = TestFS::new([
            (
                "index.js",
                "const foo = require('./foo');\nimport './bar.js';\nmodule.exports = foo;",
            ),
            ("foo.js", ""),
            ("bar.js", ""),
        ]);
        let root = fs.root();
        let logger = crate::EmptyLogger;
        let walk = crate::WalkBuilder::new(&root).build();
        let graph = crate::build_dependency_graph(&walk, None, &logger).unwrap();
        let main_idx = graph
            .node_indices()
            .find(|i| graph[*i].name == "index.js" && graph[*i].kind == NodeKind::File)
            .unwrap();
        let foo_idx = graph
            .node_indices()
            .find(|i| graph[*i].name == "foo.js" && graph[*i].kind == NodeKind::File)
            .unwrap();
        let bar_idx = graph
            .node_indices()
            .find(|i| graph[*i].name == "bar.js" && graph[*i].kind == NodeKind::File)
            .unwrap();
        assert!(graph.find_edge(main_idx, foo_idx).is_some());
        assert!(graph.find_edge(main_idx, bar_idx).is_some());
    }

    #[test]
    fn test_other_extensions() {
        let fs = TestFS::new([("a.mjs", "import './b.cjs';"), ("b.cjs", "")]);
        let root = fs.root();
        let logger = crate::EmptyLogger;
        let walk = crate::WalkBuilder::new(&root).build();
        let graph = crate::build_dependency_graph(&walk, None, &logger).unwrap();
        let a_idx = graph
            .node_indices()
            .find(|i| graph[*i].name == "a.mjs" && graph[*i].kind == NodeKind::File)
            .unwrap();
        let b_idx = graph
            .node_indices()
            .find(|i| graph[*i].name == "b.cjs" && graph[*i].kind == NodeKind::File)
            .unwrap();
        assert!(graph.find_edge(a_idx, b_idx).is_some());
    }

    proptest! {
        #[test]
        fn prop_resolve_relative_import_find(ext in proptest::sample::select(JS_EXTENSIONS)) {
            let fs = TestFS::new([(format!("dir/foo.{}", ext), "")]);
            let root = fs.root();
            let dir = root.join("dir").unwrap();
            prop_assert!(resolve_relative_import(&dir, "./foo").is_some());
        }
    }
}
