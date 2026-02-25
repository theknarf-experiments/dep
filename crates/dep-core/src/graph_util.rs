use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;

use crate::{EdgeType, Node, NodeKind};

/// Check if a node is a type singleton node
pub fn is_type_node(node: &Node) -> bool {
    node.name.starts_with("__type__::")
}

/// Resolve the NodeKind for a node by looking at its TypeOf edges.
pub fn resolve_node_kind(graph: &DiGraph<Node, EdgeType>, idx: NodeIndex) -> NodeKind {
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
