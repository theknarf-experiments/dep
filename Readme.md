# Dep

Analyze a project by building up a dependency graph between JS / TS files and exporting a Graphviz Dot file

## Usage

Run `dep` on a project directory. By default this writes `out.dot`:

```bash
cargo run -- path/to/project
```

Render the graph using Graphviz's `sfdp` for a layout suitable for large graphs:

```bash
sfdp -Goverlap=prism -Tsvg out.dot -o out.svg
```

Open `out.svg` in your browser to explore the dependency graph.

### Configuration

You can configure `dep` using a `dep.toml` file in the target directory. CLI arguments take precedence over config file settings.

Example `dep.toml`:

```toml
# Output settings
output = "graph.json"
format = "json"

# Feature toggles
include_external = false
include_builtins = true
include_folders = true
include_assets = false
include_packages = true

# Ignore specific nodes or paths
ignore_nodes = ["node_modules", "dist"]
ignore_paths = ["**/generated/**"]

# Other settings
workers = 4
verbose = true
prune = true
```
