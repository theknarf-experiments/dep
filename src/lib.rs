use petgraph::graph::{DiGraph, NodeIndex};
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use vfs::VfsPath;

pub mod output;
pub use output::{graph_to_dot, graph_to_json};
pub mod types;
use types::package_json::{PackageDepsParser, PackageMainParser};
mod analysis;
mod logger;
mod traversal;
mod tsconfig;
pub use logger::{log_error, log_verbose};
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
    if opts.verbose {
        log_verbose(opts.color, &format!("found {} files", files.len()));
    }
    let aliases = load_tsconfig_aliases(root, opts.color)?;
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
    if opts.verbose {
        log_verbose(opts.color, &format!("using {} worker threads", workers));
    }
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
    if opts.verbose {
        log_verbose(
            opts.color,
            &format!(
                "graph before prune: nodes={}, edges={}",
                res.node_count(),
                res.edge_count()
            ),
        );
    }
    if opts.prune {
        let before = res.node_count();
        analysis::prune_unconnected(&mut res);
        if opts.verbose {
            log_verbose(
                opts.color,
                &format!("pruned {} nodes", before - res.node_count()),
            );
        }
    }
    Ok(res)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::TestFS;
    use crate::types::js::JS_EXTENSIONS;
    use proptest::prelude::*;

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

    proptest! {
        #[test]
        fn prop_end_to_end(ext_a in proptest::sample::select(JS_EXTENSIONS), ext_b in proptest::sample::select(JS_EXTENSIONS)) {
            let entries = vec![
                ("proj/.gitignore".to_string(), b"ignored/".to_vec()),
                (format!("proj/src/main.{ext_a}"), format!("import '../lib/util.{ext_b}';").into_bytes()),
                (format!("proj/lib/util.{ext_b}"), Vec::new()),
                ("proj/ignored/skip.js".to_string(), Vec::new()),
            ];
            let fs = TestFS::new(entries.iter().map(|(p,c)| (p.as_str(), c.as_slice())));
            let path = fs.root().join("proj").unwrap();
            let graph = build_dependency_graph(&path, Default::default()).unwrap();

            let main_rel = format!("src/main.{ext_a}");
            let util_rel = format!("lib/util.{ext_b}");
            let main_idx = graph
                .node_indices()
                .find(|i| graph[*i].name == main_rel && graph[*i].kind == NodeKind::File)
                .unwrap();
            let util_idx = graph
                .node_indices()
                .find(|i| graph[*i].name == util_rel && graph[*i].kind == NodeKind::File)
                .unwrap();
            prop_assert!(graph.find_edge(main_idx, util_idx).is_some());
            prop_assert!(!graph.node_indices().any(|i| graph[i].name.contains("ignored")));
        }
    }
}
