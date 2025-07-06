use petgraph::graph::DiGraph;
use petgraph::visit::EdgeRef;

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
pub fn graph_to_dot(graph: &DiGraph<Node, ()>) -> String {
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
        out.push_str(&format!(
            "    {} -> {}\n",
            e.source().index(),
            e.target().index()
        ));
    }
    out.push_str("}\n");
    out
}
