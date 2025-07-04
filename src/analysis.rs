use crate::{Edge, Node};
use petgraph::graph::{DiGraph, NodeIndex};

pub fn prune_unconnected(graph: &mut DiGraph<Node, Edge>) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Edge, Node, NodeKind};
    use petgraph::graph::DiGraph;

    #[test]
    fn test_prune_unconnected() {
        let mut g: DiGraph<Node, Edge> = DiGraph::new();
        let a = g.add_node(Node {
            name: "a".into(),
            kind: NodeKind::File,
        });
        let b = g.add_node(Node {
            name: "b".into(),
            kind: NodeKind::File,
        });
        g.add_edge(a, b, Edge::default());
        let _c = g.add_node(Node {
            name: "c".into(),
            kind: NodeKind::File,
        });
        prune_unconnected(&mut g);
        assert!(g.node_indices().all(|i| g[i].name != "c"));
        assert!(g.node_indices().any(|i| g[i].name == "a"));
        assert!(g.node_indices().any(|i| g[i].name == "b"));
    }
}
