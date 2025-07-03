use clap::{Parser, ValueEnum};
use std::path::PathBuf;
use vfs::{PhysicalFS, VfsPath};

/// CLI arguments
#[derive(ValueEnum, Clone)]
enum OutputFormat {
    Dot,
    Json,
}

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

    /// Output file path
    #[arg(long, default_value = "out.dot")]
    output: PathBuf,

    /// Output format (dot or json)
    #[arg(long, value_enum, default_value_t = OutputFormat::Dot)]
    format: OutputFormat,

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
    let graph = dep::build_dependency_graph(
        &root,
        dep::BuildOptions {
            workers: args.workers,
            verbose: args.verbose,
            prune: args.prune,
            color: args.color,
        },
    )?;
    let output_str = match args.format {
        OutputFormat::Dot => dep::graph_to_dot(
            &graph,
            args.include_external,
            args.include_builtins,
            args.include_folders,
            args.include_assets,
            args.include_packages,
        ),
        OutputFormat::Json => dep::graph_to_json(
            &graph,
            args.include_external,
            args.include_builtins,
            args.include_folders,
            args.include_assets,
            args.include_packages,
        ),
    };
    std::fs::write(&args.output, output_str)?;
    Ok(())
}
