use crate::{Node, NodeKind};
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use std::collections::HashMap;

pub fn prune_unconnected(graph: &mut DiGraph<Node, ()>) {
    loop {
        let mut removed = false;
        let nodes: Vec<NodeIndex> = graph.node_indices().collect();
        for idx in nodes {
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
    graph: &DiGraph<Node, ()>,
    include_external: bool,
    include_builtin: bool,
    include_folders: bool,
    include_assets: bool,
    include_packages: bool,
    ignore_nodes: &[String],
) -> DiGraph<Node, ()> {
    let mut filtered = DiGraph::new();
    let mut map = HashMap::new();
    use std::collections::HashSet;
    let ignore: HashSet<&str> = ignore_nodes.iter().map(|s| s.as_str()).collect();
    for idx in graph.node_indices() {
        let node = &graph[idx];
        if ignore.contains(node.name.as_str()) {
            continue;
        }
        let keep = match node.kind {
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
    for edge in graph.edge_references() {
        if let (Some(&s), Some(&t)) = (map.get(&edge.source()), map.get(&edge.target())) {
            filtered.add_edge(s, t, ());
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
    use proptest::prelude::*;

    #[test]
    fn test_prune_unconnected() {
        let mut g: DiGraph<Node, ()> = DiGraph::new();
        let a = g.add_node(Node {
            name: "a".into(),
            kind: NodeKind::File,
        });
        let b = g.add_node(Node {
            name: "b".into(),
            kind: NodeKind::File,
        });
        g.add_edge(a, b, ());
        let _c = g.add_node(Node {
            name: "c".into(),
            kind: NodeKind::File,
        });
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
        let graph = build_dependency_graph(&root, None, &logger).unwrap();
        let folder_idx = graph
            .node_indices()
            .find(|i| graph[*i].name == "foo" && graph[*i].kind == NodeKind::Folder)
            .unwrap();
        let file_idx = graph
            .node_indices()
            .find(|i| graph[*i].name == "foo/bar.js" && graph[*i].kind == NodeKind::File)
            .unwrap();
        assert!(graph.find_edge(folder_idx, file_idx).is_some());

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
        let graph = build_dependency_graph(&root, None, &logger).unwrap();
        let js_idx = graph
            .node_indices()
            .find(|i| graph[*i].name == "index.js" && graph[*i].kind == NodeKind::File)
            .unwrap();
        let css_idx = graph
            .node_indices()
            .find(|i| graph[*i].name == "style.css" && graph[*i].kind == NodeKind::Asset)
            .unwrap();
        assert!(graph.find_edge(js_idx, css_idx).is_some());

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
        let graph = build_dependency_graph(&root, None, &logger).unwrap();
        let json = graph_to_json(&filter_graph(&graph, true, true, false, true, true, &[]));
        assert!(json.contains("index.js"));
        assert!(json.contains("b.js"));
    }

    #[test]
    fn test_ignore_nodes() {
        let fs = TestFS::new([("a.js", ""), ("b.js", "")]);
        let root = fs.root();
        let logger = crate::EmptyLogger;
        let graph = build_dependency_graph(&root, None, &logger).unwrap();
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

    proptest! {
        #[test]
        fn prop_filter_graph(
            include_external in any::<bool>(),
            include_builtin in any::<bool>(),
            include_folders in any::<bool>(),
            include_assets in any::<bool>(),
            include_packages in any::<bool>(),
        ) {
            let mut g = DiGraph::new();
            let file = g.add_node(Node { name: "file.js".into(), kind: NodeKind::File });
            let ext = g.add_node(Node { name: "ext".into(), kind: NodeKind::External });
            let builtin = g.add_node(Node { name: "builtin".into(), kind: NodeKind::Builtin });
            let folder = g.add_node(Node { name: "folder".into(), kind: NodeKind::Folder });
            let asset = g.add_node(Node { name: "asset.css".into(), kind: NodeKind::Asset });
            let pkg = g.add_node(Node { name: "pkg".into(), kind: NodeKind::Package });
            g.add_edge(file, ext, ());
            g.add_edge(file, builtin, ());
            g.add_edge(file, folder, ());
            g.add_edge(file, asset, ());
            g.add_edge(file, pkg, ());

            let filtered = filter_graph(
                &g,
                include_external,
                include_builtin,
                include_folders,
                include_assets,
                include_packages,
                &[],
            );

            prop_assert!(filtered.node_indices().any(|i| filtered[i].kind == NodeKind::File));
            prop_assert_eq!(filtered.node_indices().any(|i| filtered[i].kind == NodeKind::External), include_external);
            prop_assert_eq!(filtered.node_indices().any(|i| filtered[i].kind == NodeKind::Builtin), include_builtin);
            prop_assert_eq!(filtered.node_indices().any(|i| filtered[i].kind == NodeKind::Folder), include_folders);
            prop_assert_eq!(filtered.node_indices().any(|i| filtered[i].kind == NodeKind::Asset), include_assets);
            prop_assert_eq!(filtered.node_indices().any(|i| filtered[i].kind == NodeKind::Package), include_packages);
            let expected_edges = include_external as usize
                + include_builtin as usize
                + include_folders as usize
                + include_assets as usize
                + include_packages as usize;
            prop_assert_eq!(filtered.edge_count(), expected_edges);
        }
    }
}
