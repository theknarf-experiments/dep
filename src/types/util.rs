use std::path::Path;
use vfs::VfsPath;

pub(crate) const JS_EXTENSIONS: &[&str] = &["js", "jsx", "ts", "tsx", "mjs", "cjs", "mts", "cts"];

pub(crate) fn is_node_builtin(name: &str) -> bool {
    let n = name.strip_prefix("node:").unwrap_or(name);
    matches!(
        n,
        "assert"
            | "buffer"
            | "child_process"
            | "cluster"
            | "console"
            | "constants"
            | "crypto"
            | "dgram"
            | "dns"
            | "domain"
            | "events"
            | "fs"
            | "http"
            | "https"
            | "module"
            | "net"
            | "os"
            | "path"
            | "process"
            | "punycode"
            | "querystring"
            | "readline"
            | "repl"
            | "stream"
            | "string_decoder"
            | "timers"
            | "tls"
            | "tty"
            | "url"
            | "util"
            | "v8"
            | "vm"
            | "zlib"
    )
}

pub(crate) fn resolve_relative_import(dir: &VfsPath, spec: &str) -> Option<VfsPath> {
    if let Ok(base) = dir.join(spec) {
        if base.exists().ok()? {
            return Some(base);
        }
        let p = Path::new(spec);
        if p.extension().is_none() {
            for ext in JS_EXTENSIONS {
                if let Ok(candidate) = dir.join(format!("{spec}.{}", ext)) {
                    if candidate.exists().ok()? {
                        return Some(candidate);
                    }
                }
            }
            for ext in JS_EXTENSIONS {
                if let Ok(candidate) = base.join(format!("index.{}", ext)) {
                    if candidate.exists().ok()? {
                        return Some(candidate);
                    }
                }
            }
        }
    }
    None
}

pub(crate) fn resolve_alias_import(aliases: &[(String, VfsPath)], spec: &str) -> Option<VfsPath> {
    for (alias, base) in aliases {
        if spec == alias || spec.starts_with(&format!("{}/", alias)) {
            let rest = if spec == alias {
                ""
            } else {
                &spec[alias.len() + 1..]
            };
            if let Ok(candidate_base) = base.join(rest) {
                if candidate_base.exists().ok()? {
                    return Some(candidate_base);
                }
                let p = Path::new(rest);
                if p.extension().is_none() {
                    for ext in JS_EXTENSIONS {
                        if let Ok(candidate) = base.join(format!("{rest}.{}", ext)) {
                            if candidate.exists().ok()? {
                                return Some(candidate);
                            }
                        }
                    }
                    for ext in JS_EXTENSIONS {
                        if let Ok(candidate) = candidate_base.join(format!("index.{}", ext)) {
                            if candidate.exists().ok()? {
                                return Some(candidate);
                            }
                        }
                    }
                }
            }
        }
    }
    None
}
