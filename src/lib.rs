use petgraph::graph::DiGraph;
use std::sync::{Arc, Mutex};

pub use dep_core::*;
pub use dep_traversal::{Walk, WalkBuilder};
pub use dep_analysis::{filter_graph, prune_unconnected};
pub use dep_output::{graph_to_dot, graph_to_json};

pub mod output {
    pub use dep_output::*;
}

use dep_tsconfig::load_tsconfig_aliases;

/// Build a dependency graph of all JS/TS files within `root`.
pub fn build_dependency_graph(
    walk: &Walk,
    workers: Option<usize>,
    logger: &dyn Logger,
) -> anyhow::Result<DiGraph<Node, EdgeType>> {
    let files = walk.collect_files(logger)?;
    logger.log(LogLevel::Debug, &format!("found {} files", files.len()));
    let root = walk.root();
    let aliases = load_tsconfig_aliases(root, logger)?;
    let ctx = Context {
        root,
        aliases: &aliases,
        logger,
    };
    let parsers: Vec<Box<dyn Parser>> = vec![
        Box::new(dep_parser_package::PackageMainParser),
        Box::new(dep_parser_package::PackageDepsParser),
        Box::new(dep_parser_index::IndexParser),
        Box::new(dep_parser_js::JsParser),
        Box::new(dep_parser_vite::ViteParser),
        Box::new(dep_parser_mdx::MdxParser),
        Box::new(dep_parser_html::HtmlParser),
    ];
    let workers = workers.unwrap_or_else(num_cpus::get);
    logger.log(
        LogLevel::Debug,
        &format!("using {} worker threads", workers),
    );
    let edges: Arc<Mutex<Vec<Edge>>> = Arc::new(Mutex::new(Vec::new()));
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(workers)
        .build()?;
    let mut parsed_files = Vec::new();
    pool.scope(|s| {
        for path in &files {
            let parsers = &parsers;
            let ctx = &ctx;
            let edges = edges.clone();
            let path_clone = path.clone();
            let should_parse = parsers.iter().any(|p| p.can_parse(&path_clone));
            if should_parse {
                parsed_files.push(path_clone.clone());
            }
            s.spawn(move |_| {
                for p in parsers {
                    if p.can_parse(&path_clone) {
                        ctx.logger.log(
                            LogLevel::Debug,
                            &format!("Used {} parsed: {}", p.name(), path_clone.as_str()),
                        );
                        match p.parse(&path_clone, ctx) {
                            Ok(mut es) => {
                                if !es.is_empty() {
                                    match edges.lock() {
                                        Ok(mut lock) => lock.extend(es.drain(..)),
                                        Err(e) => {
                                            ctx.logger.log(
                                                LogLevel::Error,
                                                &format!("failed to lock edges mutex: {}", e),
                                            );
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                ctx.logger.log(
                                    LogLevel::Error,
                                    &format!("failed to parse {}: {}", path_clone.as_str(), e),
                                );
                            }
                        }
                    }
                }
            });
        }
    });

    let mut data = new_graph_ctx();

    // Create root folder node
    let root_idx = ensure_node("", &mut data);
    attach_type(root_idx, NodeKind::Folder, &mut data);

    let root_str = root.as_str().trim_end_matches('/');

    // Create nodes for all parsed files
    for path in &parsed_files {
        let rel = path
            .as_str()
            .strip_prefix(root_str)
            .unwrap_or(path.as_str())
            .trim_start_matches('/');
        let parent_idx = ensure_folders(rel, &mut data, root_idx);
        let idx = ensure_node(rel, &mut data);
        if data.graph.find_edge(parent_idx, idx).is_none() {
            data.graph.add_edge(parent_idx, idx, EdgeType::Regular);
        }
    }

    // Process edges from parsers
    let edges_lock = edges
        .lock()
        .map_err(|e| anyhow::anyhow!("failed to lock edges mutex: {}", e))?;
    for e in edges_lock.iter() {
        let from_idx = ensure_node(&e.from, &mut data);
        if let Some(kind) = e.from_type {
            attach_type(from_idx, kind, &mut data);
        }

        let to_idx = if e.to.contains('/') || e.to.contains('.') {
            let parent_idx = ensure_folders(&e.to, &mut data, root_idx);
            let idx = ensure_node(&e.to, &mut data);
            if data.graph.find_edge(parent_idx, idx).is_none() {
                data.graph.add_edge(parent_idx, idx, EdgeType::Regular);
            }
            idx
        } else {
            ensure_node(&e.to, &mut data)
        };

        if let Some(kind) = e.to_type {
            attach_type(to_idx, kind, &mut data);
        }

        data.graph.add_edge(from_idx, to_idx, e.kind.clone());
    }

    let res = data.graph;
    logger.log(
        LogLevel::Debug,
        &format!(
            "graph: nodes={}, edges={}",
            res.node_count(),
            res.edge_count()
        ),
    );
    Ok(res)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dep_core::test_util::TestFS;
    use dep_core::js_resolve::JS_EXTENSIONS;
    use proptest::prelude::*;

    #[test]
    fn test_build_dependency_graph_memoryfs() {
        let fs = TestFS::new([("a.js", "import './b';"), ("b.js", "")]);
        let root = fs.root();

        let logger = EmptyLogger;
        let walk = WalkBuilder::new(&root).build();
        let graph = build_dependency_graph(&walk, None, &logger).unwrap();
        let a_idx = graph
            .node_indices()
            .find(|i| graph[*i].name == "a.js")
            .unwrap();
        let b_idx = graph
            .node_indices()
            .find(|i| graph[*i].name == "b.js")
            .unwrap();
        assert!(graph.find_edge(a_idx, b_idx).is_some());
    }

    proptest! {
        #[test]
        fn prop_end_to_end(ext_a in proptest::sample::select(JS_EXTENSIONS), ext_b in proptest::sample::select(JS_EXTENSIONS)) {
            let entries = vec![
                ("proj/.gitignore".to_string(), b"ignored/".to_vec()),
                (format!("proj/src/main.{ext_a}"), format!("import '../lib/util.{ext_b}';").into_bytes()),
                (format!("proj/lib/util.{ext_b}"), Vec::new()),
                ("proj/ignored/skip.js".to_string(), Vec::new()),
            ];
            let fs = TestFS::new(entries.iter().map(|(p,c)| (p.as_str(), c.as_slice())));
            let path = fs.root().join("proj").unwrap();
            let logger = EmptyLogger;
            let walk = WalkBuilder::new(&path).build();
            let graph = build_dependency_graph(&walk, None, &logger).unwrap();

            let main_rel = format!("src/main.{ext_a}");
            let util_rel = format!("lib/util.{ext_b}");
            let main_idx = graph
                .node_indices()
                .find(|i| graph[*i].name == main_rel)
                .unwrap();
            let util_idx = graph
                .node_indices()
                .find(|i| graph[*i].name == util_rel)
                .unwrap();
            prop_assert!(graph.find_edge(main_idx, util_idx).is_some());
            prop_assert!(!graph.node_indices().any(|i| graph[i].name.contains("ignored") && !graph[i].name.starts_with("__type__")));
        }
    }
}
