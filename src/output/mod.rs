use petgraph::graph::DiGraph;
use petgraph::visit::EdgeRef;
use serde::Serialize;
use std::collections::HashMap;

use crate::{Edge, Node, NodeKind};

fn node_attrs(kind: &NodeKind) -> (&'static str, Option<&'static str>) {
    match kind {
        NodeKind::File => ("box", None),
        NodeKind::External => ("ellipse", Some("lightblue")),
        NodeKind::Builtin => ("diamond", Some("gray")),
        NodeKind::Folder => ("folder", Some("lightgrey")),
        NodeKind::Asset => ("note", Some("yellow")),
        NodeKind::Package => ("box3d", Some("orange")),
    }
}

fn escape_label(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Filter a dependency graph according to output options.
pub fn filter_graph(
    graph: &DiGraph<Node, Edge>,
    include_external: bool,
    include_builtin: bool,
    include_folders: bool,
    include_assets: bool,
    include_packages: bool,
    ignore_nodes: &[String],
) -> DiGraph<Node, Edge> {
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
            filtered.add_edge(s, t, edge.weight().clone());
        }
    }
    filtered
}

/// Convert a dependency graph to Graphviz dot format.
pub fn graph_to_dot(graph: &DiGraph<Node, Edge>) -> String {
    let mut out = String::from("digraph {\n");
    for i in graph.node_indices() {
        let node = &graph[i];
        let (shape, color) = node_attrs(&node.kind);
        let label = escape_label(&node.name);
        out.push_str(&format!(
            "    {} [label=\"{}\", shape={}",
            i.index(),
            label,
            shape
        ));
        if let Some(c) = color {
            out.push_str(&format!(", style=filled, fillcolor=\"{}\"", c));
        }
        out.push_str("]\n");
    }
    for e in graph.edge_references() {
        let mut attrs = String::new();
        if e.weight().metadata.contains_key("sameAs") {
            attrs.push_str(" [style=dashed]");
        }
        out.push_str(&format!(
            "    {} -> {}{}\n",
            e.source().index(),
            e.target().index(),
            attrs
        ));
    }
    out.push_str("}\n");
    out
}

#[derive(Serialize)]
struct JsonGraph<E> {
    nodes: Vec<Node>,
    edges: Vec<E>,
}

/// Convert a dependency graph to JSON format.
pub fn graph_to_json(graph: &DiGraph<Node, Edge>) -> String {
    #[derive(Serialize)]
    struct JsonEdge {
        source: usize,
        target: usize,
        #[serde(skip_serializing_if = "HashMap::is_empty")]
        metadata: HashMap<String, String>,
    }

    let nodes: Vec<Node> = graph.node_indices().map(|i| graph[i].clone()).collect();
    let edges: Vec<JsonEdge> = graph
        .edge_references()
        .map(|e| JsonEdge {
            source: e.source().index(),
            target: e.target().index(),
            metadata: e.weight().metadata.clone(),
        })
        .collect();
    serde_json::to_string_pretty(&JsonGraph::<JsonEdge> { nodes, edges }).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build_dependency_graph;
    use crate::test_util::TestFS;
    use proptest::prelude::*;

    #[test]
    fn test_folder_nodes() {
        let fs = TestFS::new([("foo/bar.js", "")]);
        let root = fs.root();

        let graph = build_dependency_graph(&root, Default::default()).unwrap();
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

        let graph = build_dependency_graph(&root, Default::default()).unwrap();
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
        let graph = build_dependency_graph(&root, Default::default()).unwrap();
        let json = graph_to_json(&filter_graph(&graph, true, true, false, true, true, &[]));
        assert!(json.contains("index.js"));
        assert!(json.contains("b.js"));
    }

    #[test]
    fn test_ignore_nodes() {
        let fs = TestFS::new([("a.js", ""), ("b.js", "")]);
        let root = fs.root();
        let graph = build_dependency_graph(&root, Default::default()).unwrap();
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
            let mut g: DiGraph<Node, Edge> = DiGraph::new();
            let file = g.add_node(Node { name: "file.js".into(), kind: NodeKind::File });
            let ext = g.add_node(Node { name: "ext".into(), kind: NodeKind::External });
            let builtin = g.add_node(Node { name: "builtin".into(), kind: NodeKind::Builtin });
            let folder = g.add_node(Node { name: "folder".into(), kind: NodeKind::Folder });
            let asset = g.add_node(Node { name: "asset.css".into(), kind: NodeKind::Asset });
            let pkg = g.add_node(Node { name: "pkg".into(), kind: NodeKind::Package });
            g.add_edge(file, ext, Edge::default());
            g.add_edge(file, builtin, Edge::default());
            g.add_edge(file, folder, Edge::default());
            g.add_edge(file, asset, Edge::default());
            g.add_edge(file, pkg, Edge::default());

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
