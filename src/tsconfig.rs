use serde::Deserialize;
use std::collections::HashMap;
use vfs::VfsPath;

#[derive(Deserialize)]
struct TsConfigFile {
    #[serde(rename = "compilerOptions")]
    compiler_options: Option<CompilerOptions>,
}

#[derive(Deserialize)]
struct CompilerOptions {
    #[serde(rename = "baseUrl")]
    base_url: Option<String>,
    paths: Option<HashMap<String, Vec<String>>>,
}

pub fn load_tsconfig_aliases(root: &VfsPath) -> anyhow::Result<Vec<(String, VfsPath)>> {
    if let Ok(path) = root.join("tsconfig.json") {
        if path.exists()? {
            let contents = path.read_to_string()?;
            let tsconfig: TsConfigFile = serde_json::from_str(&contents)?;
            if let Some(opts) = tsconfig.compiler_options {
                let base = opts.base_url.as_deref().unwrap_or(".");
                let base_path = root.join(base)?;
                let mut aliases = Vec::new();
                if let Some(paths) = opts.paths {
                    for (alias, targets) in paths {
                        if let Some(first) = targets.into_iter().next() {
                            let alias_prefix = alias.trim_end_matches("/*");
                            let target_prefix = first.trim_end_matches("/*");
                            if let Ok(p) = base_path.join(target_prefix) {
                                aliases.push((alias_prefix.to_string(), p));
                            }
                        }
                    }
                }
                return Ok(aliases);
            }
        }
    }
    Ok(Vec::new())
}

#[cfg(test)]
mod tests {
    use crate::test_util::TestFS;
    use crate::{NodeKind, build_dependency_graph};

    #[test]
    fn test_tsconfig_paths() {
        let fs = TestFS::new([
            (
                "tsconfig.json",
                b"{\n  \"compilerOptions\": {\n    \"baseUrl\": \".\",\n    \"paths\": { \"@foo/*\": [\"foo/*\"] }\n  }\n}" as &[u8],
            ),
            ("index.ts", b"import '@foo/bar';" as &[u8]),
            ("foo/bar.ts", b"" as &[u8]),
        ]);
        let root = fs.root();

        let graph = build_dependency_graph(&root, Default::default()).unwrap();

        let idx_index = graph
            .node_indices()
            .find(|i| graph[*i].name == "index.ts" && graph[*i].kind == NodeKind::File)
            .unwrap();
        let idx_target = graph
            .node_indices()
            .find(|i| graph[*i].name == "foo/bar.ts" && graph[*i].kind == NodeKind::File)
            .unwrap();
        assert!(graph.find_edge(idx_index, idx_target).is_some());
    }
}
