pub mod package_util;

use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use vfs::VfsPath;

use dep_core::{Context, Edge, Parser};
use dep_core::{NodeKind, EdgeType};

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
                        to_type: None,
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
    use dep_core::test_util::TestFS;

    #[test]
    fn test_package_util_parse_and_deps() {
        use crate::package_util::find_packages;

        let fs = TestFS::new([
            (
                "pkg/package.json",
                b"{\"name\":\"pkg\",\"main\":\"index.js\",\"dependencies\":{\"foo\":\"workspace:*\",\"bar\":\"1.0\"}}" as &[u8],
            ),
        ]);
        let root = fs.root();
        let logger = dep_core::EmptyLogger;
        let p = find_packages(&root, &logger).unwrap();
        assert_eq!(p.len(), 1);
        let p0 = &p[0];
        assert_eq!(p0.name, "pkg");
        assert_eq!(p0.main.as_deref(), Some("index.js"));
        assert!(p0.deps.contains(&("foo".to_string(), true)));
        assert!(p0.deps.contains(&("bar".to_string(), false)));
    }

    #[test]
    fn test_malformed_package_json() {
        use crate::package_util::find_packages;

        let fs = TestFS::new([("pkg/package.json", "not json")]);
        let root = fs.root();
        let logger = dep_core::EmptyLogger;
        let res = find_packages(&root, &logger).unwrap();
        assert!(res.is_empty());
    }
}
