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

/// Node types used for categorization and rendering.
/// These become singleton nodes in the graph that regular nodes point to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
pub enum NodeKind {
    /// Default type - no edge to type node needed
    File,
    External,
    Builtin,
    Folder,
    Asset,
    Package,
}

impl NodeKind {
    /// Returns all type variants that have singleton nodes (excludes File which is the default)
    pub fn type_node_variants() -> &'static [NodeKind] {
        &[
            NodeKind::Package,
            NodeKind::Folder,
            NodeKind::Builtin,
            NodeKind::External,
            NodeKind::Asset,
        ]
    }

    /// Returns the canonical name for this type's singleton node
    pub fn type_node_name(&self) -> &'static str {
        match self {
            NodeKind::File => "__type__::file",
            NodeKind::External => "__type__::external",
            NodeKind::Builtin => "__type__::builtin",
            NodeKind::Folder => "__type__::folder",
            NodeKind::Asset => "__type__::asset",
            NodeKind::Package => "__type__::package",
        }
    }

    /// Precedence for type resolution (higher = wins). File is 0 (default).
    pub fn precedence(&self) -> u8 {
        match self {
            NodeKind::File => 0,
            NodeKind::Asset => 1,
            NodeKind::External => 2,
            NodeKind::Builtin => 3,
            NodeKind::Folder => 4,
            NodeKind::Package => 5,
        }
    }
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

/// A node in the dependency graph, identified by its canonical name only.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct Node {
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
pub enum EdgeType {
    Regular,
    SameAs,
    /// Edge from a node to its type singleton node
    TypeOf,
}

/// Ensure a node exists in the graph, returning its index.
pub(crate) fn ensure_node(name: &str, data: &mut types::GraphCtx) -> NodeIndex {
    if let Some(&idx) = data.nodes.get(name) {
        idx
    } else {
        let idx = data.graph.add_node(Node {
            name: name.to_string(),
        });
        data.nodes.insert(name.to_string(), idx);
        idx
    }
}

/// Attach a type to a node by creating a TypeOf edge to the type singleton.
pub(crate) fn attach_type(node_idx: NodeIndex, kind: NodeKind, data: &mut types::GraphCtx) {
    if kind == NodeKind::File {
        return; // File is the default, no edge needed
    }
    if let Some(&type_idx) = data.type_nodes.get(&kind) {
        // Only add the edge if it doesn't already exist
        if data.graph.find_edge(node_idx, type_idx).is_none() {
            data.graph.add_edge(node_idx, type_idx, EdgeType::TypeOf);
        }
    }
}

/// Ensure all folder nodes exist for the given path and link them hierarchically.
/// Also attaches Folder type to each folder node.
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
            accum.push_str(&comp.as_os_str().to_string_lossy());
            let idx = ensure_node(&accum, data);
            attach_type(idx, NodeKind::Folder, data);
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
    let workers = workers.unwrap_or_else(num_cpus::get);
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
                                    match edges.lock() {
                                        Ok(mut lock) => lock.extend(es.drain(..)),
                                        Err(e) => {
                                            ctx.logger.log(
                                                LogLevel::Error,
                                                &format!("failed to lock edges mutex: {}", e),
                                            );
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                ctx.logger.log(
                                    LogLevel::Error,
                                    &format!("failed to parse {}: {}", path_clone.as_str(), e),
                                );
                            }
                        }
                    }
                }
            });
        }
    });

    let mut data = types::GraphCtx {
        graph: DiGraph::new(),
        nodes: HashMap::new(),
        type_nodes: HashMap::new(),
    };

    // Create singleton type nodes for all NodeKind variants (except File)
    for kind in NodeKind::type_node_variants() {
        let idx = data.graph.add_node(Node {
            name: kind.type_node_name().to_string(),
        });
        data.type_nodes.insert(*kind, idx);
    }

    // Create root folder node
    let root_idx = ensure_node("", &mut data);
    attach_type(root_idx, NodeKind::Folder, &mut data);

    let root_str = root.as_str().trim_end_matches('/');

    // Create nodes for all parsed files
    for path in &parsed_files {
        let rel = path
            .as_str()
            .strip_prefix(root_str)
            .unwrap_or(path.as_str())
            .trim_start_matches('/');
        let parent_idx = ensure_folders(rel, &mut data, root_idx);
        let idx = ensure_node(rel, &mut data);
        // Files don't need a TypeOf edge - File is the default
        if data.graph.find_edge(parent_idx, idx).is_none() {
            data.graph.add_edge(parent_idx, idx, EdgeType::Regular);
        }
    }

    // Process edges from parsers
    let edges_lock = edges
        .lock()
        .map_err(|e| anyhow::anyhow!("failed to lock edges mutex: {}", e))?;
    for e in edges_lock.iter() {
        // Ensure 'from' node exists and attach type if specified
        let from_idx = ensure_node(&e.from, &mut data);
        if let Some(kind) = e.from_type {
            attach_type(from_idx, kind, &mut data);
        }

        // Ensure 'to' node exists with folder structure if it looks like a path
        let to_idx = if e.to.contains('/') || e.to.contains('.') {
            // Looks like a file path - ensure folder structure
            let parent_idx = ensure_folders(&e.to, &mut data, root_idx);
            let idx = ensure_node(&e.to, &mut data);
            if data.graph.find_edge(parent_idx, idx).is_none() {
                data.graph.add_edge(parent_idx, idx, EdgeType::Regular);
            }
            idx
        } else {
            // Simple name (like a package or external dep)
            ensure_node(&e.to, &mut data)
        };

        if let Some(kind) = e.to_type {
            attach_type(to_idx, kind, &mut data);
        }

        // Add the actual dependency edge
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
            .find(|i| graph[*i].name == "a.js")
            .unwrap();
        let b_idx = graph
            .node_indices()
            .find(|i| graph[*i].name == "b.js")
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
                .find(|i| graph[*i].name == main_rel)
                .unwrap();
            let util_idx = graph
                .node_indices()
                .find(|i| graph[*i].name == util_rel)
                .unwrap();
            prop_assert!(graph.find_edge(main_idx, util_idx).is_some());
            prop_assert!(!graph.node_indices().any(|i| graph[i].name.contains("ignored") && !graph[i].name.starts_with("__type__")));
        }
    }
}
