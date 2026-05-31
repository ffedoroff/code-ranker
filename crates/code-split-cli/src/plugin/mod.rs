pub mod finalize;
pub mod javascript;
pub mod python;
pub mod rust;

use anyhow::{Result, bail};
use code_split_core::{PluginGraphs, StageTime};
use std::path::Path;

/// Run a built-in plugin for the given workspace. Returns `(graphs, timings)`.
///
/// All plugins are compiled into the binary and run in-process — there is no
/// external/dynamic plugin loading.
pub fn run(
    name: &str,
    workspace: &Path,
    local_only: bool,
) -> Result<(PluginGraphs, Vec<StageTime>)> {
    match name {
        "rust" => rust::run(workspace, local_only),
        "python" => python::run(workspace, local_only),
        "javascript" | "typescript" | "js" | "ts" => javascript::run(workspace, local_only),
        other => bail!("unknown plugin {other:?}; built-in plugins are: rust, python, javascript"),
    }
}
