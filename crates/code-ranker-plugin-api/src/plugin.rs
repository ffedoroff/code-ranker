//! The [`LanguagePlugin`] trait + [`Options`] + [`Preset`].
//!
//! A plugin is a **pure parser**: it turns a workspace into nodes + edges at
//! a requested level. It computes **no metrics** — complexity, cycles,
//! Henry-Kafura and stats are filled centrally by the orchestrator, for all
//! languages. The CLI holds the registry of plugins; it talks to them ONLY
//! through this trait and never names a concrete language.

use crate::graph::Graph;
use crate::level::{Level, Thresholds};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// Free-form key/value options passed from the CLI (future `--plugin-opt k=v`).
/// `BTreeMap` for deterministic iteration order.
pub type Options = BTreeMap<String, String>;

/// Everything the orchestrator feeds a plugin from config + CLI input.
#[derive(Debug, Clone, Default)]
pub struct PluginInput {
    /// Glob patterns for paths to skip during analysis (config + CLI).
    pub ignore: Vec<String>,
    /// When `true`, the plugin must skip its own **test files** during the walk
    /// (mirrors `[ignore] tests`). What counts as a test is language-specific —
    /// see [`LanguagePlugin::is_test_path`] — so the detection lives in the
    /// plugin, not the CLI.
    pub ignore_tests: bool,
    /// Free-form key/value options. A plugin reads its own keys, ignores the rest.
    pub options: Options,
}

/// A Prompt-Generator preset (a refactoring principle): a ready-to-paste AI
/// instruction plus how the UI seeds the node selection for it. The orchestrator
/// builds a generic default set and hands it to [`LanguagePlugin::presets`],
/// which may pass it through, edit, drop or extend per language.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preset {
    /// Stable id / short code shown on the button (e.g. `"ADP"`).
    pub id: String,
    /// Button label (usually the id).
    pub label: String,
    /// Full principle title (first heading of the generated prompt).
    pub title: String,
    /// The prompt body (Markdown, language-neutral by default).
    pub prompt: String,
    /// Link to the full principle doc, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc_url: Option<String>,
    /// The metric the recommended-node list sorts by (an attribute key, or the
    /// pseudo-metric `"cycle"`).
    pub sort_metric: String,
    /// Which connection sets the preset pre-selects: any of `"in"`/`"out"`/`"common"`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub connections: Vec<String>,
}

pub trait LanguagePlugin {
    /// Canonical name, e.g. `"rust"`. Used by `--plugin` and recorded in the
    /// snapshot. Each plugin has exactly one name (js and ts are separate).
    fn name(&self) -> &str;

    /// Can this plugin parse `workspace` (honoring `input`)?
    fn detect(&self, workspace: &Path, input: &PluginInput) -> bool;

    /// Levels this plugin can produce, each carrying its edge-kind / attribute /
    /// node-kind / cycle-kind semantics.
    fn levels(&self) -> Vec<Level>;

    /// Parse the workspace into a graph AT `level` (by name). **Structure only**:
    /// nodes (with their structural attributes) + edges. Metrics are added
    /// downstream. When `input.ignore_tests` is set, the plugin must drop its
    /// own test files here (it knows the language's conventions; see
    /// [`is_test_path`](Self::is_test_path)).
    fn analyze(&self, workspace: &Path, level: &str, input: &PluginInput) -> Result<Graph>;

    /// Does this workspace-relative path (forward-slashed, no leading `./`) name
    /// a **test** file in this language? Used to drop tests during the walk when
    /// `PluginInput::ignore_tests` is set. Default: nothing is a test.
    fn is_test_path(&self, _rel_path: &str) -> bool {
        false
    }

    /// Toolchain versions to record in the snapshot, e.g. `[("rustc", "1.88.0")]`.
    fn versions(&self, _workspace: &Path, _input: &PluginInput) -> Vec<(String, String)> {
        Vec::new()
    }

    /// Transform the orchestrator's generic default presets for this language.
    /// Default: pass them through unchanged. A plugin may reword a `prompt`,
    /// change a `sort_metric`, drop a preset, or add language-specific ones.
    fn presets(&self, defaults: Vec<Preset>, _input: &PluginInput) -> Vec<Preset> {
        defaults
    }

    /// Language-calibrated per-metric thresholds (attribute key → tiers). The
    /// orchestrator overlays these onto the attribute specs. Default: none.
    fn thresholds(&self) -> BTreeMap<String, Thresholds> {
        BTreeMap::new()
    }
}
