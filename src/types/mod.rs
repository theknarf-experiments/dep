use petgraph::graph::{DiGraph, NodeIndex};
use std::collections::HashMap;
use vfs::VfsPath;

use crate::{Logger, Node, NodeKind, EdgeType};

#[derive(Debug)]
pub struct GraphCtx {
    pub graph: DiGraph<Node, EdgeType>,
    pub nodes: HashMap<(String, NodeKind), NodeIndex>,
}

pub struct Context<'a> {
    pub root: &'a VfsPath,
    pub aliases: &'a [(String, VfsPath)],
    pub logger: &'a dyn Logger,
}

#[derive(Clone, Debug)]
pub struct Edge {
    pub from: Node,
    pub to: Node,
    pub kind: EdgeType,
}

pub trait Parser: Send + Sync {
    fn name(&self) -> &'static str;
    fn can_parse(&self, path: &VfsPath) -> bool;
    fn parse(&self, path: &VfsPath, ctx: &Context) -> anyhow::Result<Vec<Edge>>;
}

pub mod html;
pub mod js;
pub mod index;
pub mod monorepo;
pub mod package_json;
pub mod package_util;
pub mod vite;
