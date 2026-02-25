use dep_core::{EdgeType, Node, NodeKind};
use dep_core::{is_type_node, resolve_node_kind};
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use std::collections::HashMap;

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
    use dep_core::{Node, NodeKind, EdgeType};
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
    fn test_filter_graph_with_types() {
        let mut g: DiGraph<Node, EdgeType> = DiGraph::new();

        let ext_type = g.add_node(Node { name: NodeKind::External.type_node_name().into() });
        let builtin_type = g.add_node(Node { name: NodeKind::Builtin.type_node_name().into() });
        let _folder_type = g.add_node(Node { name: NodeKind::Folder.type_node_name().into() });
        let _asset_type = g.add_node(Node { name: NodeKind::Asset.type_node_name().into() });
        let _pkg_type = g.add_node(Node { name: NodeKind::Package.type_node_name().into() });

        let file = g.add_node(Node { name: "file.js".into() });
        let ext = g.add_node(Node { name: "ext".into() });
        let builtin = g.add_node(Node { name: "builtin".into() });

        g.add_edge(ext, ext_type, EdgeType::TypeOf);
        g.add_edge(builtin, builtin_type, EdgeType::TypeOf);

        g.add_edge(file, ext, EdgeType::Regular);
        g.add_edge(file, builtin, EdgeType::Regular);

        let filtered = filter_graph(&g, false, true, true, true, true, &[]);
        assert!(!filtered.node_indices().any(|i| filtered[i].name == "ext"));
        assert!(filtered.node_indices().any(|i| filtered[i].name == "builtin"));

        let filtered = filter_graph(&g, true, false, true, true, true, &[]);
        assert!(filtered.node_indices().any(|i| filtered[i].name == "ext"));
        assert!(!filtered.node_indices().any(|i| filtered[i].name == "builtin"));
    }
}
