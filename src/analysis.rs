use crate::{Node, NodeKind, EdgeType};
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use std::collections::HashMap;

/// Check if a node is a type singleton node
fn is_type_node(node: &Node) -> bool {
    node.name.starts_with("__type__::")
}

/// Resolve the NodeKind for a node by looking at its TypeOf edges.
fn resolve_node_kind(graph: &DiGraph<Node, EdgeType>, idx: NodeIndex) -> NodeKind {
    let mut best_kind = NodeKind::File;
    let mut best_precedence = 0u8;

    for edge in graph.edges(idx) {
        if *edge.weight() == EdgeType::TypeOf {
            let target = &graph[edge.target()];
            for kind in NodeKind::type_node_variants() {
                if target.name == kind.type_node_name() {
                    let prec = kind.precedence();
                    if prec > best_precedence {
                        best_precedence = prec;
                        best_kind = *kind;
                    }
                    break;
                }
            }
        }
    }

    best_kind
}

pub fn prune_unconnected(graph: &mut DiGraph<Node, EdgeType>) {
    loop {
        let mut removed = false;
        let nodes: Vec<NodeIndex> = graph.node_indices().collect();
        for idx in nodes {
            let node = &graph[idx];
            // Don't prune type singleton nodes - they may be needed for type resolution
            if is_type_node(node) {
                continue;
            }
            if graph.edges(idx).next().is_none()
                && graph
                    .edges_directed(idx, petgraph::Incoming)
                    .next()
                    .is_none()
            {
                graph.remove_node(idx);
                removed = true;
            }
        }
        if !removed {
            break;
        }
    }
}

/// Filter a dependency graph according to output options.
pub fn filter_graph(
    graph: &DiGraph<Node, EdgeType>,
    include_external: bool,
    include_builtin: bool,
    include_folders: bool,
    include_assets: bool,
    include_packages: bool,
    ignore_nodes: &[String],
) -> DiGraph<Node, EdgeType> {
    let mut filtered: DiGraph<Node, EdgeType> = DiGraph::new();
    let mut map = HashMap::new();
    use std::collections::HashSet;
    let ignore: HashSet<&str> = ignore_nodes.iter().map(|s| s.as_str()).collect();

    // First pass: add type singleton nodes (always include them for type resolution)
    for idx in graph.node_indices() {
        let node = &graph[idx];
        if is_type_node(node) {
            let nidx = filtered.add_node(node.clone());
            map.insert(idx, nidx);
        }
    }

    // Second pass: add regular nodes based on their resolved type
    for idx in graph.node_indices() {
        let node = &graph[idx];
        if is_type_node(node) {
            continue; // Already added
        }
        if ignore.contains(node.name.as_str()) {
            continue;
        }
        let kind = resolve_node_kind(graph, idx);
        let keep = match kind {
            NodeKind::External => include_external,
            NodeKind::Builtin => include_builtin,
            NodeKind::File => true,
            NodeKind::Folder => include_folders,
            NodeKind::Asset => include_assets,
            NodeKind::Package => include_packages,
        };
        if keep {
            let nidx = filtered.add_node(node.clone());
            map.insert(idx, nidx);
        }
    }

    // Add edges
    for edge in graph.edge_references() {
        if let (Some(&s), Some(&t)) = (map.get(&edge.source()), map.get(&edge.target())) {
            filtered.add_edge(s, t, edge.weight().clone());
        }
    }
    filtered
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::TestFS;
    use crate::{Node, NodeKind, build_dependency_graph, graph_to_dot, graph_to_json};
    use petgraph::graph::DiGraph;

    #[test]
    fn test_prune_unconnected() {
        let mut g: DiGraph<Node, EdgeType> = DiGraph::new();
        let a = g.add_node(Node { name: "a".into() });
        let b = g.add_node(Node { name: "b".into() });
        g.add_edge(a, b, EdgeType::Regular);
        let _c = g.add_node(Node { name: "c".into() });
        prune_unconnected(&mut g);
        assert!(g.node_indices().all(|i| g[i].name != "c"));
        assert!(g.node_indices().any(|i| g[i].name == "a"));
        assert!(g.node_indices().any(|i| g[i].name == "b"));
    }

    #[test]
    fn test_folder_nodes() {
        let fs = TestFS::new([("foo/bar.js", "")]);
        let root = fs.root();

        let logger = crate::EmptyLogger;
        let walk = crate::WalkBuilder::new(&root).build();
        let graph = build_dependency_graph(&walk, None, &logger).unwrap();
        let folder_idx = graph
            .node_indices()
            .find(|i| graph[*i].name == "foo")
            .unwrap();
        let file_idx = graph
            .node_indices()
            .find(|i| graph[*i].name == "foo/bar.js")
            .unwrap();
        assert!(graph.find_edge(folder_idx, file_idx).is_some());

        // Verify folder has Folder type
        assert_eq!(resolve_node_kind(&graph, folder_idx), NodeKind::Folder);

        let without = graph_to_dot(&filter_graph(&graph, true, true, false, true, true, &[]));
        assert!(without.contains("foo/bar.js"));
        assert!(!without.contains("shape=folder"));

        let with = graph_to_dot(&filter_graph(&graph, true, true, true, true, true, &[]));
        assert!(with.contains("shape=folder"));
    }

    #[test]
    fn test_asset_filter() {
        let fs = TestFS::new([("index.js", "import './style.css';"), ("style.css", "")]);
        let root = fs.root();

        let logger = crate::EmptyLogger;
        let walk = crate::WalkBuilder::new(&root).build();
        let graph = build_dependency_graph(&walk, None, &logger).unwrap();
        let js_idx = graph
            .node_indices()
            .find(|i| graph[*i].name == "index.js")
            .unwrap();
        let css_idx = graph
            .node_indices()
            .find(|i| graph[*i].name == "style.css")
            .unwrap();
        assert!(graph.find_edge(js_idx, css_idx).is_some());

        // Verify css has Asset type
        assert_eq!(resolve_node_kind(&graph, css_idx), NodeKind::Asset);

        let without = graph_to_dot(&filter_graph(&graph, true, true, false, false, true, &[]));
        assert!(!without.contains("style.css"));
        let with = graph_to_dot(&filter_graph(&graph, true, true, false, true, true, &[]));
        assert!(with.contains("style.css"));
    }

    #[test]
    fn test_json_output() {
        let fs = TestFS::new([("index.js", "import './b.js';"), ("b.js", "")]);
        let root = fs.root();
        let logger = crate::EmptyLogger;
        let walk = crate::WalkBuilder::new(&root).build();
        let graph = build_dependency_graph(&walk, None, &logger).unwrap();
        let json = graph_to_json(&filter_graph(&graph, true, true, false, true, true, &[]));
        assert!(json.contains("index.js"));
        assert!(json.contains("b.js"));
    }

    #[test]
    fn test_ignore_nodes() {
        let fs = TestFS::new([("a.js", ""), ("b.js", "")]);
        let root = fs.root();
        let logger = crate::EmptyLogger;
        let walk = crate::WalkBuilder::new(&root).build();
        let graph = build_dependency_graph(&walk, None, &logger).unwrap();
        let dot = graph_to_dot(&filter_graph(
            &graph,
            true,
            true,
            false,
            true,
            true,
            &["b.js".to_string()],
        ));
        assert!(dot.contains("a.js"));
        assert!(!dot.contains("b.js"));
    }

    #[test]
    fn test_filter_graph_with_types() {
        // Create a graph with type nodes
        let mut g: DiGraph<Node, EdgeType> = DiGraph::new();

        // Create type singleton nodes
        let ext_type = g.add_node(Node { name: NodeKind::External.type_node_name().into() });
        let builtin_type = g.add_node(Node { name: NodeKind::Builtin.type_node_name().into() });
        let folder_type = g.add_node(Node { name: NodeKind::Folder.type_node_name().into() });
        let asset_type = g.add_node(Node { name: NodeKind::Asset.type_node_name().into() });
        let pkg_type = g.add_node(Node { name: NodeKind::Package.type_node_name().into() });

        // Create regular nodes
        let file = g.add_node(Node { name: "file.js".into() });
        let ext = g.add_node(Node { name: "ext".into() });
        let builtin = g.add_node(Node { name: "builtin".into() });
        let folder = g.add_node(Node { name: "folder".into() });
        let asset = g.add_node(Node { name: "asset.css".into() });
        let pkg = g.add_node(Node { name: "pkg".into() });

        // Add TypeOf edges
        g.add_edge(ext, ext_type, EdgeType::TypeOf);
        g.add_edge(builtin, builtin_type, EdgeType::TypeOf);
        g.add_edge(folder, folder_type, EdgeType::TypeOf);
        g.add_edge(asset, asset_type, EdgeType::TypeOf);
        g.add_edge(pkg, pkg_type, EdgeType::TypeOf);

        // Add dependency edges
        g.add_edge(file, ext, EdgeType::Regular);
        g.add_edge(file, builtin, EdgeType::Regular);
        g.add_edge(file, folder, EdgeType::Regular);
        g.add_edge(file, asset, EdgeType::Regular);
        g.add_edge(file, pkg, EdgeType::Regular);

        // Test filtering - exclude external
        let filtered = filter_graph(&g, false, true, true, true, true, &[]);
        let dot = graph_to_dot(&filtered);
        assert!(!dot.contains("\"ext\""));
        assert!(dot.contains("builtin"));

        // Test filtering - exclude builtins
        let filtered = filter_graph(&g, true, false, true, true, true, &[]);
        let dot = graph_to_dot(&filtered);
        assert!(dot.contains("ext"));
        assert!(!dot.contains("\"builtin\""));
    }
}
