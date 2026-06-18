//! The Prompt-Generator [`Preset`] DTO.
//!
//! A `Preset` is **prompt-generator domain data**, not part of the parser
//! contract: a plugin *produces* its set (via
//! [`LanguagePlugin::presets`](crate::plugin::LanguagePlugin::presets)), but every
//! other consumer — the report snapshot, the `recommend` console/prompt views —
//! only *reads* presets and never parses anything. The type therefore lives here,
//! away from [`plugin`](crate::plugin), so those reporting consumers do not couple
//! to the parsing contract just to name this struct.

use serde::{Deserialize, Serialize};

/// The language-neutral **prompt scaffolding** the Prompt-Generator wraps a
/// [`Preset`] in — the framing prose around a principle (intro, the doc-read
/// note, the task protocol, the focus line, and the dependency-cycle note).
/// **Data, not code**: it lives in the metric catalog (`builtin.toml [prompt]`)
/// and is carried in the snapshot, so the CLI's `prompt` format and the HTML
/// viewer's Prompt Generator render the same text from one source. `{id}` in a
/// `task` line is substituted with the active preset id at render time.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PromptTemplate {
    /// One-line intent shown under the principle title.
    #[serde(default)]
    pub intro: String,
    /// Shown after the `doc_url` link: read the full principle first.
    #[serde(default)]
    pub doc_note: String,
    /// The task-protocol bullet lines (one entry per bullet).
    #[serde(default)]
    pub task: Vec<String>,
    /// The closing emphasis line.
    #[serde(default)]
    pub focus: String,
    /// Note prepended to a single dependency-cycle's module list.
    #[serde(default)]
    pub cycle_note: String,
}

/// A Prompt-Generator preset (a refactoring principle): a ready-to-paste AI
/// instruction plus how the UI seeds the node selection for it. Each plugin
/// builds its own set from config via [`LanguagePlugin::presets`](crate::plugin::LanguagePlugin::presets)
/// (the common catalog plus any language-specific presets).
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
