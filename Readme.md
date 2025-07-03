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
