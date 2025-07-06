use petgraph::graph::DiGraph;
use petgraph::visit::EdgeRef;
use serde::Serialize;

use crate::Node;

#[derive(Serialize)]
struct JsonGraph {
    nodes: Vec<Node>,
    edges: Vec<(usize, usize)>,
}

/// Convert a dependency graph to JSON format.
pub fn graph_to_json(graph: &DiGraph<Node, ()>) -> String {
    let nodes: Vec<Node> = graph.node_indices().map(|i| graph[i].clone()).collect();
    let edges: Vec<(usize, usize)> = graph
        .edge_references()
        .map(|e| (e.source().index(), e.target().index()))
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
        let graph = build_dependency_graph(&root, Default::default()).unwrap();
        let json = graph_to_json(&filter_graph(&graph, true, true, false, true, true, &[]));
        assert!(json.contains("index.js"));
        assert!(json.contains("b.js"));
    }
}
