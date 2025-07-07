use petgraph::graph::{DiGraph, NodeIndex};
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

pub mod analysis;
pub mod output;
pub use analysis::{filter_graph, prune_unconnected};
pub use output::{graph_to_dot, graph_to_json};
pub mod types;
use types::package_json::{PackageDepsParser, PackageMainParser};

mod logger;
mod traversal;
pub use traversal::{Walk, WalkBuilder};
mod tsconfig;
pub use logger::{ConsoleLogger, EmptyLogger, LogLevel, Logger};
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

impl std::fmt::Display for NodeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            NodeKind::File => "file",
            NodeKind::External => "external",
            NodeKind::Builtin => "builtin",
            NodeKind::Folder => "folder",
            NodeKind::Asset => "asset",
            NodeKind::Package => "package",
        };
        write!(f, "{}", name)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct Node {
    pub name: String,
    pub kind: Option<NodeKind>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
pub enum EdgeType {
    Regular,
    SameAs,
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
            let key = (accum.clone(), Some(NodeKind::Folder));
            let idx = if let Some(&i) = data.nodes.get(&key) {
                i
            } else {
                let i = data.graph.add_node(Node {
                    name: accum.clone(),
                    kind: Some(NodeKind::Folder),
                });
                data.nodes.insert(key.clone(), i);
                i
            };
            if data.graph.find_edge(parent_idx, idx).is_none() {
                data.graph.add_edge(parent_idx, idx, EdgeType::Regular);
            }
            parent_idx = idx;
        }
    }
    parent_idx
}

/// Build a dependency graph of all JS/TS files within `root`.
pub fn build_dependency_graph(
    walk: &Walk,
    workers: Option<usize>,
    logger: &dyn Logger,
) -> anyhow::Result<DiGraph<Node, EdgeType>> {
    let files = walk.collect_files(logger)?;
    logger.log(LogLevel::Debug, &format!("found {} files", files.len()));
    let root = walk.root();
    let aliases = load_tsconfig_aliases(root, logger)?;
    let ctx = types::Context {
        root,
        aliases: &aliases,
        logger,
    };
    let parsers: Vec<Box<dyn types::Parser>> = vec![
        Box::new(PackageMainParser),
        Box::new(PackageDepsParser),
        Box::new(types::index::IndexParser),
        Box::new(types::js::JsParser),
        Box::new(types::vite::ViteParser),
        Box::new(types::mdx::MdxParser),
        Box::new(types::html::HtmlParser),
    ];
    let workers = workers.unwrap_or_else(|| num_cpus::get());
    logger.log(
        LogLevel::Debug,
        &format!("using {} worker threads", workers),
    );
    let edges: Arc<Mutex<Vec<types::Edge>>> = Arc::new(Mutex::new(Vec::new()));
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(workers)
        .build()?;
    let mut parsed_files = Vec::new();
    pool.scope(|s| {
        for path in &files {
            let parsers = &parsers;
            let ctx = &ctx;
            let edges = edges.clone();
            let path_clone = path.clone();
            let should_parse = parsers.iter().any(|p| p.can_parse(&path_clone));
            if should_parse {
                parsed_files.push(path_clone.clone());
            }
            s.spawn(move |_| {
                for p in parsers {
                    if p.can_parse(&path_clone) {
                        ctx.logger.log(
                            LogLevel::Debug,
                            &format!("Used {} parsed: {}", p.name(), path_clone.as_str()),
                        );
                        match p.parse(&path_clone, ctx) {
                            Ok(mut es) => {
                                if !es.is_empty() {
                                    let mut lock = edges.lock().unwrap();
                                    lock.extend(es.drain(..));
                                }
                            }
                            Err(_) => {}
                        }
                    }
                }
            });
        }
    });

    let mut data = types::GraphCtx {
        graph: DiGraph::new(),
        nodes: HashMap::new(),
    };
    let root_idx = {
        let key = ("".to_string(), Some(NodeKind::Folder));
        let idx = data.graph.add_node(Node {
            name: "".to_string(),
            kind: Some(NodeKind::Folder),
        });
        data.nodes.insert(key, idx);
        idx
    };

    let root_str = root.as_str().trim_end_matches('/');
    for path in &parsed_files {
        let rel = path
            .as_str()
            .strip_prefix(root_str)
            .unwrap_or(path.as_str())
            .trim_start_matches('/');
        let parent_idx = ensure_folders(rel, &mut data, root_idx);
        let key = (rel.to_string(), Some(NodeKind::File));
        let idx = if let Some(&i) = data.nodes.get(&key) {
            i
        } else {
            let i = data.graph.add_node(Node {
                name: rel.to_string(),
                kind: Some(NodeKind::File),
            });
            data.nodes.insert(key, i);
            i
        };
        if data.graph.find_edge(parent_idx, idx).is_none() {
            data.graph.add_edge(parent_idx, idx, EdgeType::Regular);
        }
    }

    let ensure_node = |n: &Node, d: &mut types::GraphCtx| -> NodeIndex {
        match n.kind {
            Some(NodeKind::File) | Some(NodeKind::Asset) => {
                let parent_idx = ensure_folders(&n.name, d, root_idx);
                let key = (n.name.clone(), n.kind.clone());
                let idx = if let Some(&i) = d.nodes.get(&key) {
                    i
                } else {
                    let i = d.graph.add_node(n.clone());
                    d.nodes.insert(key, i);
                    i
                };
                if d.graph.find_edge(parent_idx, idx).is_none() {
                    d.graph.add_edge(parent_idx, idx, EdgeType::Regular);
                }
                idx
            }
            None => {
                // try to reuse existing node with any known kind
                if let Some((&(_, _), &idx)) = d
                    .nodes
                    .iter()
                    .find(|((name, _), _)| name == &n.name)
                {
                    idx
                } else {
                    let key = (n.name.clone(), None);
                    let i = d.graph.add_node(n.clone());
                    d.nodes.insert(key, i);
                    i
                }
            }
            _ => {
                let key = (n.name.clone(), n.kind.clone());
                if let Some(&i) = d.nodes.get(&key) {
                    i
                } else {
                    let i = d.graph.add_node(n.clone());
                    d.nodes.insert(key, i);
                    i
                }
            }
        }
    };

    for e in edges.lock().unwrap().iter() {
        let from_idx = ensure_node(&e.from, &mut data);
        let to_idx = ensure_node(&e.to, &mut data);
        data.graph.add_edge(from_idx, to_idx, e.kind.clone());
    }

    let res = data.graph;
    logger.log(
        LogLevel::Debug,
        &format!(
            "graph: nodes={}, edges={}",
            res.node_count(),
            res.edge_count()
        ),
    );
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

        let logger = EmptyLogger;
        let walk = WalkBuilder::new(&root).build();
        let graph = build_dependency_graph(&walk, None, &logger).unwrap();
        let a_idx = graph
            .node_indices()
            .find(|i| graph[*i].name == "a.js" && graph[*i].kind == Some(NodeKind::File))
            .unwrap();
        let b_idx = graph
            .node_indices()
            .find(|i| graph[*i].name == "b.js" && graph[*i].kind == Some(NodeKind::File))
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
            let logger = EmptyLogger;
            let walk = WalkBuilder::new(&path).build();
            let graph = build_dependency_graph(&walk, None, &logger).unwrap();

            let main_rel = format!("src/main.{ext_a}");
            let util_rel = format!("lib/util.{ext_b}");
            let main_idx = graph
                .node_indices()
                .find(|i| graph[*i].name == main_rel && graph[*i].kind == Some(NodeKind::File))
                .unwrap();
            let util_idx = graph
                .node_indices()
                .find(|i| graph[*i].name == util_rel && graph[*i].kind == Some(NodeKind::File))
                .unwrap();
            prop_assert!(graph.find_edge(main_idx, util_idx).is_some());
            prop_assert!(!graph.node_indices().any(|i| graph[i].name.contains("ignored")));
        }
    }
}
