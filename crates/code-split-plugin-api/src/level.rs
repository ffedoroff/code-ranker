//! Level descriptors: what a plugin can produce, plus the **semantics
//! dictionaries** that let the core handle unknown kinds/keys without hardcoding
//! their names — edge kinds ([`EdgeKindSpec`]) and node/edge
//! attribute keys ([`AttributeSpec`], grouped via [`AttributeGroup`]).
//!
//! The dictionaries are **maps** keyed by the kind/attribute/group name; the
//! spec value holds only the remaining metadata (the key is the map key).

use crate::attrs::ValueType;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Semantics of one edge kind. Keyed by the edge `kind` in
/// [`Level::edge_kinds`].
///
/// `flow` is the **single source of truth** for "is this information flow":
/// today hk/cycles use a `!= contains` blacklist while the UI uses a
/// `uses|reexports` whitelist — which diverge for any new kind. With `flow`,
/// both simply read this flag.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeKindSpec {
    /// `true` → information flow: counted in coupling/cycles AND drawn on the
    /// map. `false` → structural (e.g. `contains`): excluded from metrics and
    /// hidden on the map, but still stored.
    pub flow: bool,
    /// UI label for the connection chip (defaults to the kind when absent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// UI tooltip (none when absent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

/// A named group of attributes, for organizing them in the UI — e.g. a "loc"
/// group gathers `sloc`/`lloc`/`cloc`/`blank` under one labeled section. Keyed by
/// group name in [`Level::attribute_groups`]; attributes reference it via
/// [`AttributeSpec::group`].
///
/// This is **metadata only**: attribute storage stays flat
/// (`node.attrs["sloc"]`); grouping affects display, not access.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttributeGroup {
    /// UI label for the group (defaults to the group name when absent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// UI tooltip for the group (none when absent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

/// Describes one attribute key (on a node or an edge): its value type (so the
/// UI knows how to aggregate/format it) plus optional UI label/hint and an
/// optional group. Keyed by the attribute name in [`Level::node_attributes`] /
/// [`Level::edge_attributes`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttributeSpec {
    pub value_type: ValueType,
    /// UI label (defaults to the key when absent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// UI tooltip (none when absent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    /// Optional group this attribute belongs to, by [`AttributeGroup`] key.
    /// Ungrouped when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
}

/// An analysis level the plugin can produce, with the semantics needed to score
/// and draw it. The orchestrator asks for a level by `name`.
///
/// `name` is the plugin's own label ("file" today; "module"/"function" later) —
/// a string, not an enum, because the set of meaningful levels is
/// language-specific. The dictionaries below are maps keyed by kind/attribute/
/// group name.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Level {
    pub name: String,
    /// Edge kinds this level can contain (keyed by kind).
    pub edge_kinds: BTreeMap<String, EdgeKindSpec>,
    /// Dictionary for NODE attribute keys (keyed by attribute name). The plugin
    /// declares its structural keys; the orchestrator appends computed ones
    /// (metrics) before writing the snapshot.
    pub node_attributes: BTreeMap<String, AttributeSpec>,
    /// Dictionary for EDGE attribute keys (keyed by attribute name).
    pub edge_attributes: BTreeMap<String, AttributeSpec>,
    /// Group definitions (keyed by group name) referenced by
    /// `AttributeSpec.group`, for organizing attributes in the UI. Storage of
    /// values stays flat.
    pub attribute_groups: BTreeMap<String, AttributeGroup>,
}
