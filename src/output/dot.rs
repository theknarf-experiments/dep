use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;

use crate::{Node, NodeKind, EdgeType};

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

/// Resolve the NodeKind for a node by looking at its TypeOf edges.
/// Returns the highest precedence type, or File as default.
fn resolve_node_kind(graph: &DiGraph<Node, EdgeType>, idx: NodeIndex) -> NodeKind {
    let mut best_kind = NodeKind::File;
    let mut best_precedence = 0u8;

    for edge in graph.edges(idx) {
        if *edge.weight() == EdgeType::TypeOf {
            let target = &graph[edge.target()];
            // Check which type node this points to
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

/// Check if a node is a type singleton node (should be hidden from output)
fn is_type_node(node: &Node) -> bool {
    node.name.starts_with("__type__::")
}

/// Convert a dependency graph to Graphviz dot format.
pub fn graph_to_dot(graph: &DiGraph<Node, EdgeType>) -> String {
    let mut out = String::from("digraph {\n");
    for i in graph.node_indices() {
        let node = &graph[i];
        // Skip type singleton nodes
        if is_type_node(node) {
            continue;
        }
        let kind = resolve_node_kind(graph, i);
        let (shape, color) = node_attrs(&kind);
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
        // Skip TypeOf edges in output
        if *e.weight() == EdgeType::TypeOf {
            continue;
        }
        // Skip edges involving type nodes
        if is_type_node(&graph[e.source()]) || is_type_node(&graph[e.target()]) {
            continue;
        }
        let style = match e.weight() {
            EdgeType::SameAs => " [style=dashed]",
            _ => "",
        };
        out.push_str(&format!(
            "    {} -> {}{}\n",
            e.source().index(),
            e.target().index(),
            style
        ));
    }
    out.push_str("}\n");
    out
}
