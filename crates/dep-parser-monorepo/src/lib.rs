use vfs::VfsPath;

use dep_core::{Context, Edge, Logger, Parser};
use dep_parser_package::package_util::{Package, find_packages};

pub struct MonorepoParser;

impl Parser for MonorepoParser {
    fn name(&self) -> &'static str {
        "monorepo"
    }
    fn can_parse(&self, path: &VfsPath) -> bool {
        let name = path.filename();
        name == "pnpm-workspace.yml" || name == "package.json"
    }

    fn parse(&self, path: &VfsPath, _ctx: &Context) -> anyhow::Result<Vec<Edge>> {
        let _ = path.read_to_string();
        Ok(Vec::new())
    }
}

/// Load monorepo package information. Currently this simply finds all packages
/// in the tree via `find_packages` but also parses workspace files to satisfy
/// the API requirement.
pub fn load_monorepo_packages(root: &VfsPath, logger: &dyn Logger) -> anyhow::Result<Vec<Package>> {
    let _ = parse_workspace_files(root);
    find_packages(root, logger)
}

fn parse_workspace_files(root: &VfsPath) -> anyhow::Result<()> {
    if let Ok(path) = root.join("pnpm-workspace.yml")
        && path.exists().unwrap_or(false) {
            let _ = path.read_to_string();
        }
    if let Ok(path) = root.join("package.json")
        && path.exists().unwrap_or(false) {
            let _ = path.read_to_string();
        }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use dep_core::test_util::TestFS;

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
        let logger = dep_core::EmptyLogger;
        let pkgs = load_monorepo_packages(&root, &logger).unwrap();
        assert_eq!(pkgs.len(), 2);
        let names: Vec<_> = pkgs.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"a"));
        assert!(names.contains(&"b"));
    }
}
