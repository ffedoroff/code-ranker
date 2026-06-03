//! The [`LanguagePlugin`] trait + [`Options`].
//!
//! A plugin is a **pure parser**: it turns a workspace into nodes + edges at
//! a requested level. It computes **no metrics** — complexity, cycles,
//! Henry-Kafura and stats are filled centrally by the orchestrator, for all
//! languages. The CLI holds the registry of plugins; it talks to them ONLY
//! through this trait and never names a concrete language.
//!
//! The level descriptors a plugin returns (see [`crate::level`]) describe its
//! vocabulary, so the core handles unknown edge kinds / attribute keys
//! without hardcoding them.

use crate::graph::Graph;
use crate::level::Level;
use anyhow::Result;
use std::collections::BTreeMap;
use std::path::Path;

/// Free-form key/value options passed from the CLI (future `--plugin-opt k=v`),
/// for language-specific tuning. A plugin reads its own keys, ignores the rest.
/// `BTreeMap` for deterministic iteration order.
pub type Options = BTreeMap<String, String>;

/// Everything the orchestrator feeds a plugin from config + CLI input. Bundled
/// in one struct so the input can grow without changing trait signatures.
#[derive(Debug, Clone, Default)]
pub struct PluginInput {
    /// Glob patterns for paths to skip during analysis. Merged from config
    /// `[ignore]` and CLI `--ignore` (plus the test-files toggle expanded to
    /// patterns). The plugin must not emit nodes/edges for ignored paths.
    pub ignore: Vec<String>,
    /// Free-form key/value options (config + future `--plugin-opt k=v`). A
    /// plugin reads its own keys, ignores the rest.
    pub options: Options,
}

pub trait LanguagePlugin {
    /// Canonical name, e.g. `"rust"`. Used by `--plugin` and recorded in the
    /// snapshot. Each plugin has exactly one name (js and ts are separate
    /// plugins, not aliases).
    fn name(&self) -> &str;

    /// Can this plugin parse `workspace` (honoring `input`)? Used for
    /// `--plugin auto`: the orchestrator picks the plugin whose `detect` is true.
    fn detect(&self, workspace: &Path, input: &PluginInput) -> bool;

    /// Levels this plugin can produce, each carrying its edge-kind and
    /// attribute semantics. Today most return a single `"file"` level.
    fn levels(&self) -> Vec<Level>;

    /// Parse the workspace into a graph AT `level` (by name), honoring `input`
    /// (ignore patterns + options). **Structure only**: nodes (with their
    /// structural attributes) + edges. Metrics are added downstream. The
    /// plugin owns the projection for its language (e.g. the Rust plugin
    /// collapses its module tree to `"file"`).
    fn analyze(&self, workspace: &Path, level: &str, input: &PluginInput) -> Result<Graph>;

    /// Toolchain versions to record in the snapshot, e.g. `[("rustc", "1.88.0")]`.
    /// Default: none.
    fn versions(&self, _workspace: &Path, _input: &PluginInput) -> Vec<(String, String)> {
        Vec::new()
    }
}
