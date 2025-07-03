use colored::Colorize;
use petgraph::graph::{DiGraph, NodeIndex};
use regex::Regex;
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use swc_common::{FileName, SourceMap, sync::Lrc};
use swc_ecma_ast::{Module, ModuleDecl, ModuleItem};
use swc_ecma_parser::{EsConfig, Parser as SwcParser, StringInput, Syntax, TsConfig};
use vfs::VfsPath;

pub mod output;
pub use output::{graph_to_dot, graph_to_json};
pub mod types;
use types::package_json::{PackageDepsParser, PackageMainParser};
mod analysis;
mod traversal;
mod tsconfig;
use tsconfig::load_tsconfig_aliases;
#[cfg(test)]
pub(crate) mod test_util;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
pub enum NodeKind {
    File,
    External,
    Builtin,
    Folder,
    Asset,
    Package,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct Node {
    pub name: String,
    pub kind: NodeKind,
}

pub(crate) const JS_EXTENSIONS: &[&str] = &["js", "jsx", "ts", "tsx", "mjs", "cjs", "mts", "cts"];
pub(crate) const HTML_EXTENSIONS: &[&str] = &["html"];

#[derive(Clone, Copy)]
pub struct BuildOptions {
    pub workers: Option<usize>,
    pub verbose: bool,
    pub prune: bool,
    pub color: bool,
}

impl Default for BuildOptions {
    fn default() -> Self {
        Self {
            workers: None,
            verbose: false,
            prune: false,
            color: true,
        }
    }
}

fn log_error(color: bool, msg: &str) {
    if color {
        eprintln!("{}", msg.red());
    } else {
        eprintln!("{}", msg);
    }
}

fn log_verbose(color: bool, msg: &str) {
    if color {
        println!("{}", msg.cyan());
    } else {
        println!("{}", msg);
    }
}

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

pub(crate) fn ensure_folders(
    rel: &str,
    data: &mut types::GraphCtx,
    root_idx: NodeIndex,
) -> NodeIndex {
    let parent = Path::new(rel).parent();
    let mut parent_idx = root_idx;
    if let Some(parent) = parent {
        let mut accum = String::new();
        for comp in parent.components() {
            if !accum.is_empty() {
                accum.push('/');
            }
            accum.push_str(comp.as_os_str().to_str().unwrap());
            let key = (accum.clone(), NodeKind::Folder);
            let idx = if let Some(&i) = data.nodes.get(&key) {
                i
            } else {
                let i = data.graph.add_node(Node {
                    name: accum.clone(),
                    kind: NodeKind::Folder,
                });
                data.nodes.insert(key.clone(), i);
                i
            };
            if data.graph.find_edge(parent_idx, idx).is_none() {
                data.graph.add_edge(parent_idx, idx, ());
            }
            parent_idx = idx;
        }
    }
    parent_idx
}

/// Parse a JS/TS file and return the list of relative imports.
pub fn parse_file(path: &VfsPath, color: bool) -> anyhow::Result<Vec<String>> {
    let src = match path.read_to_string() {
        Ok(s) => s,
        Err(e) => {
            log_error(color, &format!("failed to read {}: {e}", path.as_str()));
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

/// Build a dependency graph of all JS/TS files within `root`.
pub fn build_dependency_graph(
    root: &VfsPath,
    opts: BuildOptions,
) -> anyhow::Result<DiGraph<Node, ()>> {
    let data = Arc::new(Mutex::new(types::GraphCtx {
        graph: DiGraph::new(),
        nodes: HashMap::new(),
    }));
    let root_idx = {
        let mut d = data.lock().unwrap();
        let key = ("".to_string(), NodeKind::Folder);
        if let Some(&idx) = d.nodes.get(&key) {
            idx
        } else {
            let idx = d.graph.add_node(Node {
                name: "".to_string(),
                kind: NodeKind::Folder,
            });
            d.nodes.insert(key, idx);
            idx
        }
    };

    let files = traversal::collect_files(root, opts.color)?;
    let aliases = load_tsconfig_aliases(root)?;
    let ctx = types::Context {
        data: data.clone(),
        root_idx,
        root,
        aliases: &aliases,
        color: opts.color,
    };
    let parsers: Vec<Box<dyn types::Parser>> = vec![
        Box::new(PackageMainParser),
        Box::new(PackageDepsParser),
        Box::new(types::js::JsParser),
        Box::new(types::html::HtmlParser),
    ];
    let workers = opts.workers.unwrap_or_else(|| num_cpus::get());
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(workers)
        .build()?;
    pool.scope(|s| {
        for path in files {
            let parsers = &parsers;
            let ctx = &ctx;
            let verbose = opts.verbose;
            s.spawn(move |_| {
                if verbose {
                    log_verbose(ctx.color, &format!("file: {}", path.as_str()));
                }
                for p in parsers {
                    if p.can_parse(&path) {
                        if verbose {
                            log_verbose(ctx.color, &format!("  parser {}", p.name()));
                        }
                        let _ = p.parse(&path, ctx);
                    }
                }
            });
        }
    });
    drop(ctx);
    let mut res = Arc::try_unwrap(data).unwrap().into_inner().unwrap().graph;
    if opts.prune {
        analysis::prune_unconnected(&mut res);
    }
    Ok(res)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::TestFS;
    use proptest::prelude::*;
    use swc_common::FileName;

    #[test]
    fn test_collect_imports_from_string() {
        let src =
            "import foo from './foo';\nexport * from './bar';\nexport { baz } from './baz.js';";
        let module = parse_module(src, "js", FileName::Custom("test.js".into())).unwrap();
        let imports = collect_imports(&module);
        assert_eq!(
            imports,
            vec!["./foo", "./bar", "./baz.js"]
                .into_iter()
                .map(String::from)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_build_dependency_graph_memoryfs() {
        let fs = TestFS::new([("a.js", "import './b';"), ("b.js", "")]);
        let root = fs.root();

        let graph = build_dependency_graph(&root, Default::default()).unwrap();
        let a_idx = graph
            .node_indices()
            .find(|i| graph[*i].name == "a.js" && graph[*i].kind == NodeKind::File)
            .unwrap();
        let b_idx = graph
            .node_indices()
            .find(|i| graph[*i].name == "b.js" && graph[*i].kind == NodeKind::File)
            .unwrap();
        assert!(graph.find_edge(a_idx, b_idx).is_some());
    }

    #[test]
    fn test_recursive_with_gitignore() {
        let fs = TestFS::new([
            (".gitignore", "ignored.js\n"),
            ("foo/a.js", "import '../bar/b.js';\nimport 'fs';"),
            ("bar/b.js", ""),
            ("ignored.js", ""),
        ]);
        let root = fs.root();

        let graph = build_dependency_graph(&root, Default::default()).unwrap();
        // Ensure ignored file is not present
        assert!(graph.node_indices().all(|i| graph[i].name != "ignored.js"));

        // Ensure recursive import edge exists
        let a_idx = graph
            .node_indices()
            .find(|i| graph[*i].name == "foo/a.js" && graph[*i].kind == NodeKind::File)
            .unwrap();
        let b_idx = graph
            .node_indices()
            .find(|i| graph[*i].name == "bar/b.js" && graph[*i].kind == NodeKind::File)
            .unwrap();
        assert!(graph.find_edge(a_idx, b_idx).is_some());

        // check builtin node
        assert!(
            graph
                .node_indices()
                .any(|i| graph[i].name == "fs" && matches!(graph[i].kind, NodeKind::Builtin))
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

        let graph = build_dependency_graph(&root, Default::default()).unwrap();
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

        let graph = build_dependency_graph(&root, Default::default()).unwrap();

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

        let graph = build_dependency_graph(&root, Default::default()).unwrap();
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

        let graph = build_dependency_graph(&root, Default::default()).unwrap();
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

    #[test]
    fn test_package_nodes_and_edges() {
        let fs = TestFS::new([
            (
                "packages/a/package.json",
                b"{\"name\":\"a\",\"main\":\"index.js\",\"dependencies\":{\"b\":\"workspace:*\",\"ext\":\"1\"}}" as &[u8]
            ),
            ("packages/a/index.js", b"" as &[u8]),
            ("packages/b/package.json", b"{\"name\":\"b\"}" as &[u8]),
        ]);
        let root = fs.root();

        let graph = build_dependency_graph(&root, Default::default()).unwrap();
        let a_idx = graph
            .node_indices()
            .find(|i| graph[*i].name == "a" && graph[*i].kind == NodeKind::Package)
            .unwrap();
        let b_idx = graph
            .node_indices()
            .find(|i| graph[*i].name == "b" && graph[*i].kind == NodeKind::Package)
            .unwrap();
        let main_idx = graph
            .node_indices()
            .find(|i| graph[*i].name == "packages/a/index.js" && graph[*i].kind == NodeKind::File)
            .unwrap();
        assert!(graph.find_edge(a_idx, b_idx).is_some());
        assert!(graph.find_edge(a_idx, main_idx).is_some());
        assert!(
            graph
                .node_indices()
                .any(|i| graph[i].name == "ext" && graph[i].kind == NodeKind::External)
        );
    }

    #[test]
    fn test_malformed_package_json_is_ignored() {
        let fs = TestFS::new([("pkg/package.json", "notjson")]);
        let root = fs.root();
        let res = build_dependency_graph(&root, Default::default());
        assert!(res.is_ok());
    }

    proptest! {
        #[test]
        fn prop_resolve_relative_import_find(ext in proptest::sample::select(JS_EXTENSIONS)) {
            let fs = TestFS::new([
                (format!("dir/foo.{}", ext), ""),
            ]);
            let root = fs.root();
            let dir = root.join("dir").unwrap();
            prop_assert!(resolve_relative_import(&dir, "./foo").is_some());
        }
    }
}
