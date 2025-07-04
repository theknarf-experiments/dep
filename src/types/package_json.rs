use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use vfs::VfsPath;

use crate::types::{Context, Parser};
use crate::{Node, NodeKind, ensure_folders};

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

    fn parse(&self, path: &VfsPath, ctx: &Context) -> anyhow::Result<()> {
        let Some(raw) = read_package(path)? else {
            return Ok(());
        };
        let Some(name) = raw.name else {
            return Ok(());
        };
        let pkg_idx;
        {
            let mut data = ctx.data.lock().unwrap();
            let key = (name.clone(), NodeKind::Package);
            pkg_idx = if let Some(&i) = data.nodes.get(&key) {
                i
            } else {
                let i = data.graph.add_node(Node {
                    name: name.clone(),
                    kind: NodeKind::Package,
                });
                data.nodes.insert(key, i);
                i
            };
        }
        if let Some(main) = raw.main {
            if let Ok(main_path) = path.parent().join(&main) {
                if main_path.exists().unwrap_or(false) {
                    let root_str = ctx.root.as_str().trim_end_matches('/');
                    let rel = main_path
                        .as_str()
                        .strip_prefix(root_str)
                        .unwrap_or(main_path.as_str())
                        .trim_start_matches('/')
                        .to_string();
                    let mut data = ctx.data.lock().unwrap();
                    let parent_idx = ensure_folders(&rel, &mut data, ctx.root_idx);
                    let key = (rel.clone(), NodeKind::File);
                    let file_idx = if let Some(&i) = data.nodes.get(&key) {
                        i
                    } else {
                        let i = data.graph.add_node(Node {
                            name: rel.clone(),
                            kind: NodeKind::File,
                        });
                        data.nodes.insert(key, i);
                        i
                    };
                    if data.graph.find_edge(parent_idx, file_idx).is_none() {
                        data.graph
                            .add_edge(parent_idx, file_idx, crate::Edge::default());
                    }
                    data.graph
                        .add_edge(pkg_idx, file_idx, crate::Edge::default());
                }
            }
        }
        Ok(())
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

    fn parse(&self, path: &VfsPath, ctx: &Context) -> anyhow::Result<()> {
        let Some(raw) = read_package(path)? else {
            return Ok(());
        };
        let Some(name) = raw.name else {
            return Ok(());
        };
        let pkg_idx;
        {
            let mut data = ctx.data.lock().unwrap();
            let key = (name.clone(), NodeKind::Package);
            pkg_idx = if let Some(&i) = data.nodes.get(&key) {
                i
            } else {
                let i = data.graph.add_node(Node {
                    name: name.clone(),
                    kind: NodeKind::Package,
                });
                data.nodes.insert(key, i);
                i
            };
        }

        let mut deps = HashMap::new();
        if let Some(map) = raw.dependencies {
            deps.extend(map.into_iter());
        }
        if let Some(map) = raw.dev_dependencies {
            deps.extend(map.into_iter());
        }

        for (dep, ver) in deps {
            let workspace = ver.starts_with("workspace:");
            if workspace {
                let mut data = ctx.data.lock().unwrap();
                let key = (dep.clone(), NodeKind::Package);
                let to_idx = if let Some(&i) = data.nodes.get(&key) {
                    i
                } else {
                    let i = data.graph.add_node(Node {
                        name: dep.clone(),
                        kind: NodeKind::Package,
                    });
                    data.nodes.insert(key, i);
                    i
                };
                data.graph.add_edge(pkg_idx, to_idx, crate::Edge::default());
            } else {
                let mut data = ctx.data.lock().unwrap();
                let key = (dep.clone(), NodeKind::External);
                let to_idx = if let Some(&i) = data.nodes.get(&key) {
                    i
                } else {
                    let i = data.graph.add_node(Node {
                        name: dep.clone(),
                        kind: NodeKind::External,
                    });
                    data.nodes.insert(key, i);
                    i
                };
                data.graph.add_edge(pkg_idx, to_idx, crate::Edge::default());
            }
        }
        Ok(())
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
        let graph = crate::build_dependency_graph(&root, Default::default()).unwrap();
        assert!(graph.node_indices().any(|i| graph[i].name == "pkg"));
    }

    #[test]
    fn test_package_parsers_malformed() {
        let fs = TestFS::new([("pkg/package.json", "not json")]);
        let root = fs.root();
        let res = crate::build_dependency_graph(&root, Default::default());
        assert!(res.is_ok());
    }

    #[test]
    fn test_malformed_package_json_is_ignored() {
        let fs = TestFS::new([("pkg/package.json", "notjson")]);
        let root = fs.root();
        let res = crate::build_dependency_graph(&root, Default::default());
        assert!(res.is_ok());
    }
}
