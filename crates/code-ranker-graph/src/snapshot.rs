//! The serializable analysis artifact ([`Snapshot`]) and its header types
//! ([`GitInfo`], [`StageTime`]).
//!
//! Shape (schema version `"2"`): the snapshot keeps the historical header
//! (workspace/target/plugin/roots/versions/git/timings) and carries a `graphs`
//! map `level_name -> LevelGraph`. The per-level payload lives in
//! [`crate::level_graph`]; canonical serialization in [`crate::serialize`]; id
//! relativization in [`crate::relativize`].

use crate::level_graph::LevelGraph;
use chrono::{DateTime, Utc};
use code_ranker_plugin_api::Preset;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The snapshot schema version this build produces and can read back. A
/// `--baseline` (or snapshot input) with a different version is rejected with a
/// structured error rather than silently mis-parsed.
pub const SCHEMA_VERSION: &str = "2";

/// Per-stage timing in milliseconds, in execution order.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageTime {
    pub stage: String,
    pub ms: u64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub schema_version: String,
    pub generated_at: DateTime<Utc>,
    pub command: String,
    /// Directory from which `code-ranker` was invoked.
    pub workspace: String,
    /// The analyzed project directory (absolute path, stored once here).
    pub target: String,
    pub plugin: String,
    /// Config file used for this analysis, if any was found.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_file: Option<String>,
    pub versions: BTreeMap<String, String>,
    /// Named system roots used to shorten node paths (e.g. `{registry}`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub roots: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git: Option<GitInfo>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub timings: Vec<StageTime>,
    /// Analysis levels, keyed by level name. Today only `"files"` is produced.
    pub graphs: BTreeMap<String, LevelGraph>,
    /// Prompt-Generator presets (refactoring principles), language-adapted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub presets: Vec<Preset>,
    /// Prompt-Generator scaffolding prose (language-neutral framing), so the CLI
    /// `prompt` format and the HTML viewer render the same text from one source.
    #[serde(default)]
    pub prompt: code_ranker_plugin_api::PromptTemplate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitInfo {
    pub branch: String,
    pub commit: String,
    pub dirty_files: u32,
    /// Remote `origin` URL (raw). Used by the HTML report for source links.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
}

impl Snapshot {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        command: String,
        workspace: String,
        target: String,
        plugin: String,
        config_file: Option<String>,
        versions: BTreeMap<String, String>,
        roots: BTreeMap<String, String>,
        git: Option<GitInfo>,
        timings: Vec<StageTime>,
        graphs: BTreeMap<String, LevelGraph>,
        presets: Vec<Preset>,
        prompt: code_ranker_plugin_api::PromptTemplate,
    ) -> Self {
        Self {
            schema_version: SCHEMA_VERSION.to_string(),
            generated_at: Utc::now(),
            command,
            workspace,
            target,
            plugin,
            config_file,
            versions,
            roots,
            git,
            timings,
            graphs,
            presets,
            prompt,
        }
    }
}
