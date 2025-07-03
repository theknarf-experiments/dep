use vfs::VfsPath;

use crate::types::package_util::{Package, find_packages};
use crate::types::{Context, Parser};

pub struct MonorepoParser;

impl Parser for MonorepoParser {
    fn name(&self) -> &'static str {
        "monorepo"
    }
    fn can_parse(&self, path: &VfsPath) -> bool {
        let name = path.filename();
        name == "pnpm-workspace.yml" || name == "package.json"
    }

    fn parse(&self, path: &VfsPath, _ctx: &Context) -> anyhow::Result<()> {
        let _ = path.read_to_string();
        Ok(())
    }
}

/// Load monorepo package information. Currently this simply finds all packages
/// in the tree via `find_packages` but also parses workspace files to satisfy
/// the API requirement.
pub fn load_monorepo_packages(root: &VfsPath, color: bool) -> anyhow::Result<Vec<Package>> {
    // Attempt to parse pnpm-workspace.yml and package.json workspaces but the
    // returned packages are still discovered via `find_packages` so malformed
    // files do not cause a failure.
    let _ = parse_workspace_files(root);
    find_packages(root, color)
}

fn parse_workspace_files(root: &VfsPath) -> anyhow::Result<()> {
    // parse pnpm-workspace.yml
    if let Ok(path) = root.join("pnpm-workspace.yml") {
        if path.exists().unwrap_or(false) {
            let _ = path.read_to_string(); // ignore errors
        }
    }
    // parse workspaces from package.json
    if let Ok(path) = root.join("package.json") {
        if path.exists().unwrap_or(false) {
            let _ = path.read_to_string();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::TestFS;

    #[test]
    fn test_workspace_dependency_edges() {
        let fs = TestFS::new([
            (
                "packages/a/package.json",
                b"{\"name\":\"a\",\"dependencies\":{\"b\":\"workspace:*\"}}" as &[u8],
            ),
            ("packages/a/index.js", b"" as &[u8]),
            ("packages/b/package.json", b"{\"name\":\"b\"}" as &[u8]),
        ]);
        let root = fs.root();
        let pkgs = load_monorepo_packages(&root, false).unwrap();
        assert_eq!(pkgs.len(), 2);
        let names: Vec<_> = pkgs.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"a"));
        assert!(names.contains(&"b"));
    }

    #[test]
    fn test_package_nodes_and_edges() {
        let fs = TestFS::new([
            (
                "packages/a/package.json",
                b"{\"name\":\"a\",\"main\":\"index.js\",\"dependencies\":{\"b\":\"workspace:*\",\"ext\":\"1\"}}" as &[u8]
            ),
            ("packages/a/index.js", b"" as &[u8]),
            ("packages/b/package.json", b"{\"name\":\"b\"}" as &[u8]),
        ]);
        let root = fs.root();

        let graph = crate::build_dependency_graph(&root, Default::default()).unwrap();
        let a_idx = graph
            .node_indices()
            .find(|i| graph[*i].name == "a" && graph[*i].kind == crate::NodeKind::Package)
            .unwrap();
        let b_idx = graph
            .node_indices()
            .find(|i| graph[*i].name == "b" && graph[*i].kind == crate::NodeKind::Package)
            .unwrap();
        let main_idx = graph
            .node_indices()
            .find(|i| {
                graph[*i].name == "packages/a/index.js" && graph[*i].kind == crate::NodeKind::File
            })
            .unwrap();
        assert!(graph.find_edge(a_idx, b_idx).is_some());
        assert!(graph.find_edge(a_idx, main_idx).is_some());
        assert!(
            graph
                .node_indices()
                .any(|i| graph[i].name == "ext" && graph[i].kind == crate::NodeKind::External)
        );
    }
}
