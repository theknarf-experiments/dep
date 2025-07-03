use jsonc_parser::ParseOptions;
use jsonc_parser::parse_to_serde_value;
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

pub fn load_tsconfig_aliases(
    root: &VfsPath,
    color: bool,
) -> anyhow::Result<Vec<(String, VfsPath)>> {
    if let Ok(path) = root.join("tsconfig.json") {
        if path.exists()? {
            let contents = match path.read_to_string() {
                Ok(c) => c,
                Err(e) => {
                    crate::log_error(color, &format!("failed to read {}: {e}", path.as_str()));
                    return Ok(Vec::new());
                }
            };
            let tsconfig: TsConfigFile =
                match parse_to_serde_value(&contents, &ParseOptions::default()) {
                    Ok(Some(value)) => match serde_json::from_value(value) {
                        Ok(v) => v,
                        Err(e) => {
                            crate::log_error(color, &format!("failed to parse tsconfig.json: {e}"));
                            return Ok(Vec::new());
                        }
                    },
                    Ok(None) => TsConfigFile {
                        compiler_options: None,
                    },
                    Err(e) => {
                        crate::log_error(color, &format!("failed to parse tsconfig.json: {e}"));
                        return Ok(Vec::new());
                    }
                };
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

    #[test]
    fn test_tsconfig_jsonc_with_comments() {
        let fs = TestFS::new([
            (
                "tsconfig.json",
                b"{\n  // comment\n  \"compilerOptions\": {\n    /* base */ \"baseUrl\": \".\",\n    \"paths\": { \"@foo/*\": [\"foo/*\",] }\n  }\n}" as &[u8],
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

    #[test]
    fn test_alias_and_relative_refer_to_same_file() {
        let fs = TestFS::new([
            (
                "tsconfig.json",
                b"{\n  \"compilerOptions\": {\n    \"baseUrl\": \".\",\n    \"paths\": { \"@lib/*\": [\"lib/*\"] }\n  }\n}" as &[u8],
            ),
            ("a.ts", b"import './lib/c';" as &[u8]),
            ("b.ts", b"import '@lib/c';" as &[u8]),
            ("lib/c.ts", b"" as &[u8]),
        ]);
        let root = fs.root();

        let graph = build_dependency_graph(&root, Default::default()).unwrap();

        let idx_a = graph
            .node_indices()
            .find(|i| graph[*i].name == "a.ts" && graph[*i].kind == NodeKind::File)
            .unwrap();
        let idx_b = graph
            .node_indices()
            .find(|i| graph[*i].name == "b.ts" && graph[*i].kind == NodeKind::File)
            .unwrap();
        let idx_c = graph
            .node_indices()
            .find(|i| graph[*i].name == "lib/c.ts" && graph[*i].kind == NodeKind::File)
            .unwrap();

        let file_nodes: Vec<_> = graph
            .node_indices()
            .filter(|i| graph[*i].kind == NodeKind::File)
            .collect();
        assert_eq!(file_nodes.len(), 3);

        assert!(graph.find_edge(idx_a, idx_c).is_some());
        assert!(graph.find_edge(idx_b, idx_c).is_some());
    }

    #[test]
    fn test_malformed_tsconfig_does_not_fail() {
        let fs = TestFS::new([("tsconfig.json", "not json"), ("index.ts", "")]);
        let root = fs.root();
        let res = build_dependency_graph(&root, Default::default());
        assert!(res.is_ok());
    }
}
