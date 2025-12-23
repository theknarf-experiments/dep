use clap::Parser;
use dep::output::OutputType;
use dep::{LogLevel, Logger};
use std::path::PathBuf;
use vfs::{PhysicalFS, VfsPath};

/// CLI arguments

#[derive(Parser)]
#[command(
    name = "dep",
    about = "Analyze JS/TS dependencies and output Graphviz dot or json",
    after_help = "Environment variables:\n  CI - disable color output by default"
)]
struct Args {
    /// Path of the project to analyze
    #[arg(default_value = ".")]
    path: PathBuf,

    /// Include external packages in output
    #[arg(long, default_value_t = true)]
    include_external: bool,

    /// Include node builtins in output
    #[arg(long, default_value_t = true)]
    include_builtins: bool,

    /// Include folder nodes in output
    #[arg(long, default_value_t = false)]
    include_folders: bool,

    /// Include imported asset files (e.g. CSS) in output
    #[arg(long, default_value_t = true)]
    include_assets: bool,

    /// Include package nodes in output
    #[arg(long, default_value_t = true)]
    include_packages: bool,

    /// Node names to ignore from output
    #[arg(long = "ignore-node")]
    ignore_nodes: Vec<String>,

    /// File or folder patterns to ignore when scanning
    #[arg(long = "ignore", name = "PATTERN")]
    ignore_paths: Vec<String>,

    /// Output file path
    #[arg(long, default_value = "out.dot")]
    output: PathBuf,

    /// Output format (dot or json)
    #[arg(long, value_enum, default_value_t = OutputType::Dot)]
    format: OutputType,

    /// Limit worker threads
    #[arg(long)]
    workers: Option<usize>,

    /// Verbose output
    #[arg(long, default_value_t = false)]
    verbose: bool,

    /// Colored output
    #[arg(long, default_value_t = default_color())]
    color: bool,

    /// Prune nodes without edges
    #[arg(long, default_value_t = false)]
    prune: bool,
}

fn default_color() -> bool {
    std::env::var("CI").map(|v| v.is_empty()).unwrap_or(true)
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let root: VfsPath = PhysicalFS::new(&args.path).into();
    let logger = dep::ConsoleLogger {
        color: args.color,
        verbose: args.verbose,
    };
    let walk = dep::WalkBuilder::new(&root)
        .ignore_patterns(&args.ignore_paths)
        .build();
    let mut graph = dep::build_dependency_graph(&walk, args.workers, &logger)?;
    if args.prune {
        let before = graph.node_count();
        dep::prune_unconnected(&mut graph);
        logger.log(
            LogLevel::Debug,
            &format!("pruned {} nodes", before - graph.node_count()),
        );
    }
    let filtered = dep::filter_graph(
        &graph,
        args.include_external,
        args.include_builtins,
        args.include_folders,
        args.include_assets,
        args.include_packages,
        &args.ignore_nodes,
    );
    use dep::{NodeKind, EdgeType};
    use petgraph::visit::EdgeRef;
    use std::collections::HashMap;

    // Helper to resolve node kind from TypeOf edges
    fn resolve_kind(graph: &petgraph::graph::DiGraph<dep::Node, dep::EdgeType>, idx: petgraph::graph::NodeIndex) -> NodeKind {
        let mut best_kind = NodeKind::File;
        let mut best_prec = 0u8;
        for edge in graph.edges(idx) {
            if *edge.weight() == EdgeType::TypeOf {
                let target = &graph[edge.target()];
                for kind in NodeKind::type_node_variants() {
                    if target.name == kind.type_node_name() {
                        let prec = kind.precedence();
                        if prec > best_prec {
                            best_prec = prec;
                            best_kind = *kind;
                        }
                        break;
                    }
                }
            }
        }
        best_kind
    }

    fn is_type_node(node: &dep::Node) -> bool {
        node.name.starts_with("__type__::")
    }

    let mut counts: HashMap<NodeKind, (usize, usize)> = HashMap::new();
    for idx in filtered.node_indices() {
        if is_type_node(&filtered[idx]) {
            continue;
        }
        let kind = resolve_kind(&filtered, idx);
        counts.entry(kind).or_default().0 += 1;
    }
    for e in filtered.edge_references() {
        if *e.weight() == EdgeType::TypeOf {
            continue;
        }
        if is_type_node(&filtered[e.source()]) || is_type_node(&filtered[e.target()]) {
            continue;
        }
        let kind = resolve_kind(&filtered, e.source());
        counts.entry(kind).or_default().1 += 1;
    }
    let output_str = dep::output::graph_to_string(args.format, &filtered);
    std::fs::write(&args.output, &output_str)?;
    println!("Saving {} file {}", args.format, args.output.display());
    for kind in &[
        NodeKind::File,
        NodeKind::External,
        NodeKind::Builtin,
        NodeKind::Folder,
        NodeKind::Asset,
        NodeKind::Package,
    ] {
        let (nodes, edges) = counts.get(kind).cloned().unwrap_or((0, 0));
        println!("{}: {} nodes & {} edges", kind, nodes, edges);
    }
    Ok(())
}
