use petgraph::graph::NodeIndex;
use std::path::Path;

use crate::{EdgeType, GraphCtx, Node, NodeKind};

/// Ensure a node exists in the graph, returning its index.
pub fn ensure_node(name: &str, data: &mut GraphCtx) -> NodeIndex {
    if let Some(&idx) = data.nodes.get(name) {
        idx
    } else {
        let idx = data.graph.add_node(Node {
            name: name.to_string(),
        });
        data.nodes.insert(name.to_string(), idx);
        idx
    }
}

/// Attach a type to a node by creating a TypeOf edge to the type singleton.
pub fn attach_type(node_idx: NodeIndex, kind: NodeKind, data: &mut GraphCtx) {
    if kind == NodeKind::File {
        return; // File is the default, no edge needed
    }
    if let Some(&type_idx) = data.type_nodes.get(&kind) {
        // Only add the edge if it doesn't already exist
        if data.graph.find_edge(node_idx, type_idx).is_none() {
            data.graph.add_edge(node_idx, type_idx, EdgeType::TypeOf);
        }
    }
}

/// Ensure all folder nodes exist for the given path and link them hierarchically.
/// Also attaches Folder type to each folder node.
pub fn ensure_folders(
    rel: &str,
    data: &mut GraphCtx,
    root_idx: NodeIndex,
) -> NodeIndex {
    let parent = Path::new(rel).parent();
    let mut parent_idx = root_idx;
    if let Some(parent) = parent {
        let mut accum = String::new();
        for comp in parent.components() {
            if !accum.is_empty() {
                accum.push('/');
            }
            accum.push_str(&comp.as_os_str().to_string_lossy());
            let idx = ensure_node(&accum, data);
            attach_type(idx, NodeKind::Folder, data);
            if data.graph.find_edge(parent_idx, idx).is_none() {
                data.graph.add_edge(parent_idx, idx, EdgeType::Regular);
            }
            parent_idx = idx;
        }
    }
    parent_idx
}
