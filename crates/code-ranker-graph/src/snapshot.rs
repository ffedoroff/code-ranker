//! The serializable analysis artifact ([`Snapshot`]) and its header types
//! ([`GitInfo`], [`StageTime`]).
//!
//! Shape (schema version `"5"`): the snapshot keeps the historical header
//! (workspace/target/plugins/roots/versions/git/timings) and carries a `languages`
//! map `lang_name -> LanguageSnapshot`, each of which holds the per-language
//! graphs, principles, and prompt template.  The per-level payload lives in
//! [`crate::level_graph`]; canonical serialization in [`crate::serialize`]; id
//! relativization in [`crate::relativize`].

use crate::level_graph::LevelGraph;
use chrono::{DateTime, Utc};
use code_ranker_plugin_api::Principle;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The JSON-snapshot + viewer format version (re-exported from [`crate::version`]).
/// Written as `schema_version`, rejected on mismatch in `analyze.rs`, and checked
/// in the viewer. See `docs/versions.md`.
pub use crate::version::SCHEMA_VERSION;

/// Per-stage timing in milliseconds, in execution order.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageTime {
    pub stage: String,
    pub ms: u64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub detail: String,
}

/// Per-language analysis output stored inside the snapshot.
///
/// Each active language plugin contributes one entry in [`Snapshot::languages`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanguageSnapshot {
    /// Analysis levels for this language, keyed by level name (e.g. `"files"`,
    /// `"functions"`).
    pub graphs: BTreeMap<String, LevelGraph>,
    /// Prompt-Generator principles (refactoring principles), language-adapted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub principles: Vec<Principle>,
    /// Prompt-Generator scaffolding prose (language-neutral framing), so the CLI
    /// `prompt` format and the HTML viewer render the same text from one source.
    #[serde(default)]
    pub prompt: code_ranker_plugin_api::PromptTemplate,
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
    /// Sorted list of active plugin names for this analysis run.
    pub plugins: Vec<String>,
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
    /// Per-language analysis results, keyed by plugin name.
    pub languages: BTreeMap<String, LanguageSnapshot>,
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

/// Named-field constructor for [`Snapshot`], replacing the old 12-argument
/// positional function.  All callers should fill every field explicitly; use
/// `Default::default()` for truly optional ones.
pub struct SnapshotInit {
    pub command: String,
    pub workspace: String,
    pub target: String,
    /// Sorted list of active plugin names.
    pub plugins: Vec<String>,
    pub config_file: Option<String>,
    pub versions: BTreeMap<String, String>,
    pub roots: BTreeMap<String, String>,
    pub git: Option<GitInfo>,
    pub timings: Vec<StageTime>,
    pub languages: BTreeMap<String, LanguageSnapshot>,
}

impl Snapshot {
    pub fn new(init: SnapshotInit) -> Self {
        Self {
            schema_version: SCHEMA_VERSION.to_string(),
            generated_at: Utc::now(),
            command: init.command,
            workspace: init.workspace,
            target: init.target,
            plugins: init.plugins,
            config_file: init.config_file,
            versions: init.versions,
            roots: init.roots,
            git: init.git,
            timings: init.timings,
            languages: init.languages,
        }
    }
}
