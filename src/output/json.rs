use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use serde::Serialize;

use crate::{Node, NodeKind, EdgeType};

#[derive(Serialize)]
struct JsonNode {
    name: String,
    kind: NodeKind,
}

#[derive(Serialize)]
struct JsonEdge {
    from: usize,
    to: usize,
    #[serde(rename = "type")]
    kind: EdgeType,
}

#[derive(Serialize)]
struct JsonGraph {
    nodes: Vec<JsonNode>,
    edges: Vec<JsonEdge>,
}

/// Check if a node is a type singleton node (should be hidden from output)
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

/// Convert a dependency graph to JSON format.
pub fn graph_to_json(graph: &DiGraph<Node, EdgeType>) -> String {
    use std::collections::HashMap;

    // Build a mapping from old indices to new indices (excluding type nodes)
    let mut index_map: HashMap<usize, usize> = HashMap::new();
    let mut nodes: Vec<JsonNode> = Vec::new();

    for i in graph.node_indices() {
        let node = &graph[i];
        if is_type_node(node) {
            continue;
        }
        let kind = resolve_node_kind(graph, i);
        index_map.insert(i.index(), nodes.len());
        nodes.push(JsonNode {
            name: node.name.clone(),
            kind,
        });
    }

    let edges: Vec<JsonEdge> = graph
        .edge_references()
        .filter(|e| {
            // Skip TypeOf edges
            if *e.weight() == EdgeType::TypeOf {
                return false;
            }
            // Skip edges involving type nodes
            let src = &graph[e.source()];
            let tgt = &graph[e.target()];
            !is_type_node(src) && !is_type_node(tgt)
        })
        .filter_map(|e| {
            let from = index_map.get(&e.source().index())?;
            let to = index_map.get(&e.target().index())?;
            Some(JsonEdge {
                from: *from,
                to: *to,
                kind: e.weight().clone(),
            })
        })
        .collect();

    serde_json::to_string_pretty(&JsonGraph { nodes, edges }).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build_dependency_graph;
    use crate::filter_graph;
    use crate::test_util::TestFS;

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
}
