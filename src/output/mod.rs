pub mod dot;
pub mod json;

use clap::ValueEnum;

#[derive(Clone, Copy, ValueEnum)]
pub enum OutputType {
    Dot,
    Json,
}

impl std::fmt::Display for OutputType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            OutputType::Dot => "dot",
            OutputType::Json => "json",
        };
        write!(f, "{}", s)
    }
}

pub use dot::graph_to_dot;
pub use json::graph_to_json;

use crate::Node;
use petgraph::graph::DiGraph;

/// Render the dependency graph in the requested [`OutputType`].
pub fn graph_to_string(format: OutputType, graph: &DiGraph<Node, ()>) -> String {
    match format {
        OutputType::Dot => graph_to_dot(graph),
        OutputType::Json => graph_to_json(graph),
    }
}
