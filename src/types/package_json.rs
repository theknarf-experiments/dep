use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use vfs::VfsPath;

use crate::types::{Context, Edge, Parser};
use crate::{NodeKind, EdgeType};

#[derive(Deserialize)]
struct RawPackage {
    name: Option<String>,
    main: Option<String>,
    dependencies: Option<HashMap<String, String>>,
    #[serde(rename = "devDependencies")]
    dev_dependencies: Option<HashMap<String, String>>,
}

fn read_package(path: &VfsPath) -> anyhow::Result<Option<RawPackage>> {
    let content = path.read_to_string()?;
    Ok(serde_json::from_str(&content).ok())
}

pub struct PackageMainParser;

impl Parser for PackageMainParser {
    fn name(&self) -> &'static str {
        "package_main"
    }
    fn can_parse(&self, path: &VfsPath) -> bool {
        Path::new(path.as_str())
            .file_name()
            .and_then(|s| s.to_str())
            == Some("package.json")
    }

    fn parse(&self, path: &VfsPath, ctx: &Context) -> anyhow::Result<Vec<Edge>> {
        let Some(raw) = read_package(path)? else {
            return Ok(Vec::new());
        };
        let Some(name) = raw.name else {
            return Ok(Vec::new());
        };
        let mut edges = Vec::new();
        if let Some(main) = raw.main
            && let Ok(main_path) = path.parent().join(&main)
                && main_path.exists().unwrap_or(false) {
                    let root_str = ctx.root.as_str().trim_end_matches('/');
                    let rel = main_path
                        .as_str()
                        .strip_prefix(root_str)
                        .unwrap_or(main_path.as_str())
                        .trim_start_matches('/')
                        .to_string();
                    edges.push(Edge {
                        from: name.clone(),
                        to: rel,
                        kind: EdgeType::Regular,
                        from_type: Some(NodeKind::Package),
                        to_type: None, // File is default
                    });
                }
        Ok(edges)
    }
}

pub struct PackageDepsParser;

impl Parser for PackageDepsParser {
    fn name(&self) -> &'static str {
        "package_deps"
    }
    fn can_parse(&self, path: &VfsPath) -> bool {
        Path::new(path.as_str())
            .file_name()
            .and_then(|s| s.to_str())
            == Some("package.json")
    }

    fn parse(&self, path: &VfsPath, _ctx: &Context) -> anyhow::Result<Vec<Edge>> {
        let Some(raw) = read_package(path)? else {
            return Ok(Vec::new());
        };
        let Some(name) = raw.name else {
            return Ok(Vec::new());
        };
        let mut edges = Vec::new();

        let mut deps = HashMap::new();
        if let Some(map) = raw.dependencies {
            deps.extend(map);
        }
        if let Some(map) = raw.dev_dependencies {
            deps.extend(map);
        }

        for (dep, ver) in deps {
            let workspace = ver.starts_with("workspace:");
            let to_type = if workspace {
                Some(NodeKind::Package)
            } else {
                Some(NodeKind::External)
            };
            edges.push(Edge {
                from: name.clone(),
                to: dep.clone(),
                kind: EdgeType::Regular,
                from_type: Some(NodeKind::Package),
                to_type,
            });
        }
        Ok(edges)
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::TestFS;

    #[test]
    fn test_package_parsers_basic() {
        let fs = TestFS::new([
            (
                "pkg/package.json",
                b"{\"name\":\"pkg\",\"main\":\"index.js\"}" as &[u8],
            ),
            ("pkg/index.js", b"" as &[u8]),
        ]);
        let root = fs.root();
        let logger = crate::EmptyLogger;
        let walk = crate::WalkBuilder::new(&root).build();
        let graph = crate::build_dependency_graph(&walk, None, &logger).unwrap();
        assert!(graph.node_indices().any(|i| graph[i].name == "pkg"));
    }

    #[test]
    fn test_package_parsers_malformed() {
        let fs = TestFS::new([("pkg/package.json", "not json")]);
        let root = fs.root();
        let logger = crate::EmptyLogger;
        let walk = crate::WalkBuilder::new(&root).build();
        let res = crate::build_dependency_graph(&walk, None, &logger);
        assert!(res.is_ok());
    }

    #[test]
    fn test_malformed_package_json_is_ignored() {
        let fs = TestFS::new([("pkg/package.json", "notjson")]);
        let root = fs.root();
        let logger = crate::EmptyLogger;
        let walk = crate::WalkBuilder::new(&root).build();
        let res = crate::build_dependency_graph(&walk, None, &logger);
        assert!(res.is_ok());
    }
}
