use petgraph::graph::DiGraph;
use petgraph::visit::EdgeRef;
use serde::Serialize;

use dep_core::{Node, NodeKind, EdgeType};
use dep_core::{is_type_node, resolve_node_kind};

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

/// Convert a dependency graph to JSON format.
pub fn graph_to_json(graph: &DiGraph<Node, EdgeType>) -> String {
    use std::collections::HashMap;

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
            if *e.weight() == EdgeType::TypeOf {
                return false;
            }
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
