mod crate_graph;
mod ids;
mod module_graph;

use anyhow::{Context, Result};
use cargo_metadata::MetadataCommand;
use code_split_core::GraphBuilder;
use std::path::Path;

pub fn analyze(workspace: &Path, builder: &mut GraphBuilder) -> Result<()> {
    analyze_with(workspace, builder, false)
}

/// Variant of [`analyze`] that passes `--no-deps` to `cargo metadata`.
/// External crates are not enumerated and `metadata.resolve` is `None`,
/// so the resulting graph contains only the workspace's local crates,
/// their modules, files, and traits — no external crate nodes and no
/// crate-level `Uses` edges into externals.
///
/// Use when third-party dependencies are unavailable (e.g. private git
/// deps without credentials) or when the analysis intentionally
/// focuses on the local code.
pub fn analyze_local_only(workspace: &Path, builder: &mut GraphBuilder) -> Result<()> {
    analyze_with(workspace, builder, true)
}

fn analyze_with(workspace: &Path, builder: &mut GraphBuilder, local_only: bool) -> Result<()> {
    let manifest = workspace.join("Cargo.toml");
    let mut cmd = MetadataCommand::new();
    cmd.manifest_path(&manifest);
    if local_only {
        cmd.other_options(["--no-deps".to_string()]);
    }
    let metadata = cmd
        .exec()
        .with_context(|| format!("running cargo metadata for {}", manifest.display()))?;

    crate_graph::contribute(&metadata, builder);
    module_graph::contribute(&metadata, builder)?;
    Ok(())
}
