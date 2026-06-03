//! A [`Edge`] — a directed edge between two nodes (an import, a call, a
//! containment, …). The `kind` is the plugin's own vocabulary; its semantics
//! (flow vs structural, label, hint) come from a matching
//! [`EdgeKindSpec`](crate::EdgeKindSpec).

use crate::attrs::Attributes;
use crate::node::NodeId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub source: NodeId,
    pub target: NodeId,
    /// The plugin's vocabulary — "uses"/"contains"/"reexports"/… today;
    /// "calls"/"reads"/"writes"/… later. Not interpreted by the core.
    pub kind: String,
    /// Free-form attributes (e.g. `external`, or language-specific keys),
    /// described by the level's `edge_attributes` dictionary. Flattened into
    /// the edge JSON object.
    #[serde(flatten)]
    pub attrs: Attributes,
}
