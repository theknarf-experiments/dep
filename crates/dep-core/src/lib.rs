pub mod types;
pub mod graph;
pub mod graph_util;
pub mod logger;
pub mod js_resolve;
#[cfg(feature = "testutil")]
pub mod test_util;

pub use logger::{ConsoleLogger, EmptyLogger, LogLevel, Logger};
pub use types::{Context, Edge, GraphCtx, Parser};
pub use graph::{attach_type, ensure_folders, ensure_node};
pub use graph_util::{is_type_node, resolve_node_kind};

use petgraph::graph::DiGraph;
use serde::Serialize;

/// Node types used for categorization and rendering.
/// These become singleton nodes in the graph that regular nodes point to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
pub enum NodeKind {
    /// Default type - no edge to type node needed
    File,
    External,
    Builtin,
    Folder,
    Asset,
    Package,
}

impl NodeKind {
    /// Returns all type variants that have singleton nodes (excludes File which is the default)
    pub fn type_node_variants() -> &'static [NodeKind] {
        &[
            NodeKind::Package,
            NodeKind::Folder,
            NodeKind::Builtin,
            NodeKind::External,
            NodeKind::Asset,
        ]
    }

    /// Returns the canonical name for this type's singleton node
    pub fn type_node_name(&self) -> &'static str {
        match self {
            NodeKind::File => "__type__::file",
            NodeKind::External => "__type__::external",
            NodeKind::Builtin => "__type__::builtin",
            NodeKind::Folder => "__type__::folder",
            NodeKind::Asset => "__type__::asset",
            NodeKind::Package => "__type__::package",
        }
    }

    /// Precedence for type resolution (higher = wins). File is 0 (default).
    pub fn precedence(&self) -> u8 {
        match self {
            NodeKind::File => 0,
            NodeKind::Asset => 1,
            NodeKind::External => 2,
            NodeKind::Builtin => 3,
            NodeKind::Folder => 4,
            NodeKind::Package => 5,
        }
    }
}

impl std::fmt::Display for NodeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            NodeKind::File => "file",
            NodeKind::External => "external",
            NodeKind::Builtin => "builtin",
            NodeKind::Folder => "folder",
            NodeKind::Asset => "asset",
            NodeKind::Package => "package",
        };
        write!(f, "{}", name)
    }
}

/// A node in the dependency graph, identified by its canonical name only.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct Node {
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
pub enum EdgeType {
    Regular,
    SameAs,
    /// Edge from a node to its type singleton node
    TypeOf,
}

/// Initialize type singleton nodes in a GraphCtx.
pub fn init_type_nodes(data: &mut GraphCtx) {
    for kind in NodeKind::type_node_variants() {
        let idx = data.graph.add_node(Node {
            name: kind.type_node_name().to_string(),
        });
        data.type_nodes.insert(*kind, idx);
    }
}

/// Create a new empty GraphCtx with type singleton nodes pre-initialized.
pub fn new_graph_ctx() -> GraphCtx {
    let mut data = GraphCtx {
        graph: DiGraph::new(),
        nodes: std::collections::HashMap::new(),
        type_nodes: std::collections::HashMap::new(),
    };
    init_type_nodes(&mut data);
    data
}
