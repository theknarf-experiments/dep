use petgraph::graph::{DiGraph, NodeIndex};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use vfs::VfsPath;

use crate::{Node, NodeKind};

#[derive(Debug)]
pub struct GraphCtx {
    pub graph: DiGraph<Node, ()>,
    pub nodes: HashMap<(String, NodeKind), NodeIndex>,
}

pub struct Context<'a> {
    pub data: Arc<Mutex<GraphCtx>>,
    pub root_idx: NodeIndex,
    pub root: &'a VfsPath,
    pub aliases: &'a [(String, VfsPath)],
    pub color: bool,
}

pub trait Parser: Send + Sync {
    fn name(&self) -> &'static str;
    fn can_parse(&self, path: &VfsPath) -> bool;
    fn parse(&self, path: &VfsPath, ctx: &Context) -> anyhow::Result<()>;
}

pub mod html;
pub mod js;
pub mod monorepo;
pub mod package_json;
pub mod package_util;
