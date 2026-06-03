//! The [`Node`] — anything we analyze. Today a source file (`kind == "file"`),
//! later a folder / module / function / variable / line, with no model change.

use crate::attrs::Attributes;
use serde::{Deserialize, Serialize};

/// Stable string key for a node. Scheme is the plugin's choice, e.g.
/// `file:{path}` for a source file, `ext:{name}` for an external library.
pub type NodeId = String;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    /// The plugin's own vocabulary — "file" today; "module"/"function"/… later.
    /// The core never interprets this, only stores and projects on it.
    pub kind: String,
    pub name: String,
    /// Containing node, if any (a hard structural link to another node by id —
    /// e.g. a function's file).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<NodeId>,
    /// Free-form attributes (`path`, `loc`, `visibility`, `version`, …, plus
    /// language-specific keys). The plugin fills structural ones; the
    /// orchestrator adds computed ones (metrics, cycle) into the same map.
    /// Described by the level's `node_attributes` dictionary. Flattened into the
    /// node JSON object.
    #[serde(flatten)]
    pub attrs: Attributes,
}
