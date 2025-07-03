use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use vfs::{VfsFileType, VfsPath};

#[derive(Debug)]
pub struct Package {
    pub name: String,
    pub dir: VfsPath,
    pub main: Option<String>,
    pub deps: Vec<(String, bool)>, // (package name, workspace?)
}

#[derive(Deserialize)]
struct RawPackage {
    name: Option<String>,
    main: Option<String>,
    dependencies: Option<HashMap<String, String>>,
    #[serde(rename = "devDependencies")]
    dev_dependencies: Option<HashMap<String, String>>,
}

fn parse_package_file(path: &VfsPath) -> anyhow::Result<Option<Package>> {
    let content = path.read_to_string()?;
    let raw: RawPackage = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };
    let name = match raw.name {
        Some(n) => n,
        None => return Ok(None),
    };
    let main = raw.main;
    let mut deps = Vec::new();
    if let Some(map) = raw.dependencies {
        for (k, v) in map {
            let ws = v.starts_with("workspace:");
            deps.push((k, ws));
        }
    }
    if let Some(map) = raw.dev_dependencies {
        for (k, v) in map {
            let ws = v.starts_with("workspace:");
            deps.push((k, ws));
        }
    }
    let dir = path.parent();
    Ok(Some(Package {
        name,
        dir,
        main,
        deps,
    }))
}

/// Find all packages under `root` by looking for package.json files.
pub fn find_packages(root: &VfsPath, color: bool) -> anyhow::Result<Vec<Package>> {
    let mut list = Vec::new();
    let walk = match root.walk_dir() {
        Ok(w) => w,
        Err(e) => {
            crate::log_error(color, &format!("failed to walk {}: {e}", root.as_str()));
            return Ok(list);
        }
    };
    for entry in walk {
        let path = match entry {
            Ok(p) => p,
            Err(e) => {
                crate::log_error(color, &format!("walk error: {e}"));
                continue;
            }
        };
        let meta = match path.metadata() {
            Ok(m) => m,
            Err(e) => {
                crate::log_error(color, &format!("metadata error on {}: {e}", path.as_str()));
                continue;
            }
        };
        if meta.file_type == VfsFileType::Directory {
            continue;
        }
        if Path::new(path.as_str())
            .file_name()
            .and_then(|s| s.to_str())
            != Some("package.json")
        {
            continue;
        }
        if path.as_str().contains("node_modules/") {
            continue;
        }
        if let Ok(Some(pkg)) = parse_package_file(&path) {
            list.push(pkg);
        }
    }
    Ok(list)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::TestFS;

    #[test]
    fn test_parse_package_and_dependencies() {
        let fs = TestFS::new([
            (
                "pkg/package.json",
                b"{\"name\":\"pkg\",\"main\":\"index.js\",\"dependencies\":{\"foo\":\"workspace:*\",\"bar\":\"1.0\"}}" as &[u8],
            ),
        ]);
        let root = fs.root();
        let p = find_packages(&root, false).unwrap();
        assert_eq!(p.len(), 1);
        let p0 = &p[0];
        assert_eq!(p0.name, "pkg");
        assert_eq!(p0.main.as_deref(), Some("index.js"));
        assert!(p0.deps.contains(&("foo".to_string(), true)));
        assert!(p0.deps.contains(&("bar".to_string(), false)));
    }

    #[test]
    fn test_malformed_package_json() {
        let fs = TestFS::new([("pkg/package.json", "not json")]);
        let root = fs.root();
        let res = find_packages(&root, false).unwrap();
        assert!(res.is_empty());
    }
}
