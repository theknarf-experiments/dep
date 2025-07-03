use petgraph::graph::DiGraph;
use petgraph::visit::EdgeRef;
use serde::Serialize;
use std::collections::HashMap;

use crate::{Node, NodeKind};

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

/// Convert a dependency graph to Graphviz dot format.
pub fn graph_to_dot(
    graph: &DiGraph<Node, ()>,
    include_external: bool,
    include_builtin: bool,
    include_folders: bool,
    include_assets: bool,
    include_packages: bool,
) -> String {
    let mut filtered = DiGraph::new();
    let mut map = HashMap::new();
    for idx in graph.node_indices() {
        let node = &graph[idx];
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

    let mut out = String::from("digraph {\n");
    for i in filtered.node_indices() {
        let node = &filtered[i];
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
    for e in filtered.edge_references() {
        out.push_str(&format!(
            "    {} -> {}\n",
            e.source().index(),
            e.target().index()
        ));
    }
    out.push_str("}\n");
    out
}

#[derive(Serialize)]
struct JsonGraph {
    nodes: Vec<Node>,
    edges: Vec<(usize, usize)>,
}

/// Convert a dependency graph to JSON format.
pub fn graph_to_json(
    graph: &DiGraph<Node, ()>,
    include_external: bool,
    include_builtin: bool,
    include_folders: bool,
    include_assets: bool,
    include_packages: bool,
) -> String {
    let mut filtered = DiGraph::new();
    let mut map = HashMap::new();
    for idx in graph.node_indices() {
        let node = &graph[idx];
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
    let nodes: Vec<Node> = filtered
        .node_indices()
        .map(|i| filtered[i].clone())
        .collect();
    let edges: Vec<(usize, usize)> = filtered
        .edge_references()
        .map(|e| (e.source().index(), e.target().index()))
        .collect();
    serde_json::to_string_pretty(&JsonGraph { nodes, edges }).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build_dependency_graph;
    use crate::test_util::TestFS;

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

        let without = graph_to_dot(&graph, true, true, false, true, true);
        assert!(without.contains("foo/bar.js"));
        assert!(!without.contains("shape=folder"));

        let with = graph_to_dot(&graph, true, true, true, true, true);
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

        let without = graph_to_dot(&graph, true, true, false, false, true);
        assert!(!without.contains("style.css"));
        let with = graph_to_dot(&graph, true, true, false, true, true);
        assert!(with.contains("style.css"));
    }

    #[test]
    fn test_json_output() {
        let fs = TestFS::new([("index.js", "import './b.js';"), ("b.js", "")]);
        let root = fs.root();
        let graph = build_dependency_graph(&root, Default::default()).unwrap();
        let json = graph_to_json(&graph, true, true, false, true, true);
        assert!(json.contains("index.js"));
        assert!(json.contains("b.js"));
    }
}
