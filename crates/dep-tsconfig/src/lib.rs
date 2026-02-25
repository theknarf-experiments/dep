use jsonc_parser::ParseOptions;
use jsonc_parser::parse_to_serde_value;
use serde::Deserialize;
use std::collections::HashMap;
use vfs::VfsPath;

use dep_core::{LogLevel, Logger};

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
    logger: &dyn Logger,
) -> anyhow::Result<Vec<(String, VfsPath)>> {
    if let Ok(path) = root.join("tsconfig.json")
        && path.exists()? {
            let contents = match path.read_to_string() {
                Ok(c) => c,
                Err(e) => {
                    logger.log(
                        LogLevel::Error,
                        &format!("failed to read {}: {e}", path.as_str()),
                    );
                    return Ok(Vec::new());
                }
            };
            let tsconfig: TsConfigFile =
                match parse_to_serde_value(&contents, &ParseOptions::default()) {
                    Ok(Some(value)) => match serde_json::from_value(value) {
                        Ok(v) => v,
                        Err(e) => {
                            logger.log(
                                LogLevel::Error,
                                &format!("failed to parse tsconfig.json: {e}"),
                            );
                            return Ok(Vec::new());
                        }
                    },
                    Ok(None) => TsConfigFile {
                        compiler_options: None,
                    },
                    Err(e) => {
                        logger.log(
                            LogLevel::Error,
                            &format!("failed to parse tsconfig.json: {e}"),
                        );
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
    Ok(Vec::new())
}
