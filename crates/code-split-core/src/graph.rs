use serde::{Deserialize, Serialize};
use std::collections::HashSet;

fn round_sig3(x: f64) -> f64 {
    if !x.is_finite() {
        return 0.0; // NaN / ±Inf → 0 (JSON has no NaN, serde_json would emit null)
    }
    if x == 0.0 {
        return 0.0;
    }
    let abs = x.abs();
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let truncated = if abs >= 1.0 {
        // truncate to 3 decimal places
        (abs * 1000.0).floor() / 1000.0
    } else {
        // truncate to 3 significant digits after leading zeros (3 sig figs total)
        let d = abs.log10().floor() as i32;
        let factor = 10f64.powi(2 - d);
        (abs * factor).floor() / factor
    };
    truncated * sign
}

fn sig3<S: serde::Serializer>(v: &f64, s: S) -> Result<S::Ok, S::Error> {
    let x = round_sig3(*v);
    if x.fract() == 0.0 && x.abs() < i64::MAX as f64 {
        s.serialize_i64(x as i64)
    } else {
        s.serialize_f64(x)
    }
}

fn is_zero_f64(v: &f64) -> bool {
    *v == 0.0
}

fn is_zero_u32(v: &u32) -> bool {
    *v == 0
}

pub type NodeId = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    /// A source file — the only node kind that appears in snapshot output.
    File,
    /// An external dependency (library / crate), recorded at depth 1: one node
    /// per library, never expanded into its internals.
    External,
    /// Internal-only kinds used by the Rust syntactic stage (`code-split-syn`)
    /// while building the module tree. They are collapsed into `File`/`External`
    /// by the Rust plugin before the snapshot is written and never serialized.
    Crate,
    Module,
    Trait,
}

/// Structural cycle kind assigned to every node that participates in an SCC
/// of size ≥ 2 in its projected graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CycleKind {
    /// Rust-specific: a `#[cfg(test)] mod tests { use super::* }` pattern.
    /// The parent→child `contains` edge combined with the child→parent `uses`
    /// edge forms a cycle that is a language feature, not an architecture smell.
    TestEmbed,
    /// Two nodes that directly depend on each other (SCC size = 2, no test node).
    Mutual,
    /// Three or more nodes in a dependency cycle (no test node).
    Chain,
}

/// One strongly-connected component with ≥ 2 nodes, together with its
/// classification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CycleGroup {
    pub kind: CycleKind,
    pub nodes: Vec<NodeId>,
}

/// Coupling averages stored in `GraphStats` (f64 counterpart of `Coupling`).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AvgCoupling {
    #[serde(default, serialize_with = "sig3", skip_serializing_if = "is_zero_f64")]
    pub fan_in: f64,
    #[serde(default, serialize_with = "sig3", skip_serializing_if = "is_zero_f64")]
    pub fan_out: f64,
    #[serde(default, serialize_with = "sig3", skip_serializing_if = "is_zero_f64")]
    pub hk: f64,
}

/// Per-graph average metrics, mirroring the `complexity` node structure.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GraphStats {
    #[serde(default, serialize_with = "sig3", skip_serializing_if = "is_zero_f64")]
    pub cyclomatic: f64,
    #[serde(default, serialize_with = "sig3", skip_serializing_if = "is_zero_f64")]
    pub cognitive: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coupling: Option<AvgCoupling>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maintainability: Option<Maintainability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loc: Option<Loc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub halstead: Option<Halstead>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    /// Internal-only: structural ownership in the Rust module tree. Used during
    /// construction/collapse; not present between files in snapshot output.
    Contains,
    Uses,
    Reexports,
}

/// Visibility of a node. Serialised as a plain string for simple variants,
/// or as `{"restricted": "<path>"}` for the `Restricted` variant.
///
/// Deserialisation supports both:
///   - new format: `"public"`, `"private"`, `"crate"`, `"super"`,
///     `{"restricted": "some::path"}`
///   - old (tagged) format: `{"kind": "public"}`, `{"kind": "restricted",
///     "path": "some::path"}`
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Visibility {
    Public,
    Crate,
    Super,
    Restricted { path: String },
    Private,
}

impl Serialize for Visibility {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        match self {
            Visibility::Public => s.serialize_str("public"),
            Visibility::Private => s.serialize_str("private"),
            Visibility::Crate => s.serialize_str("crate"),
            Visibility::Super => s.serialize_str("super"),
            Visibility::Restricted { path } => {
                let mut map = s.serialize_map(Some(1))?;
                map.serialize_entry("restricted", path)?;
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for Visibility {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::{self, MapAccess, Visitor};
        use std::fmt;

        struct VisVisitor;

        impl<'de> Visitor<'de> for VisVisitor {
            type Value = Visibility;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(
                    r#"a visibility string ("public", "private", "crate", "super") \
                    or an object {"restricted": "<path>"} / {"kind": "<kind>", ...}"#,
                )
            }

            // New format: plain string
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Visibility, E> {
                match v {
                    "public" => Ok(Visibility::Public),
                    "private" => Ok(Visibility::Private),
                    "crate" => Ok(Visibility::Crate),
                    "super" => Ok(Visibility::Super),
                    other => Err(E::unknown_variant(
                        other,
                        &["public", "private", "crate", "super"],
                    )),
                }
            }

            // Object format — handles both new `{"restricted": "..."}` and old
            // tagged `{"kind": "public"}` / `{"kind": "restricted", "path": "..."}`
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Visibility, A::Error> {
                let mut kind: Option<String> = None;
                let mut path: Option<String> = None;
                let mut restricted: Option<String> = None;

                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        // new format key
                        "restricted" => restricted = Some(map.next_value()?),
                        // old tagged-enum keys
                        "kind" => kind = Some(map.next_value()?),
                        "path" => path = Some(map.next_value()?),
                        _ => {
                            let _: serde::de::IgnoredAny = map.next_value()?;
                        }
                    }
                }

                if let Some(r) = restricted {
                    return Ok(Visibility::Restricted { path: r });
                }

                match kind.as_deref() {
                    Some("public") => Ok(Visibility::Public),
                    Some("private") => Ok(Visibility::Private),
                    Some("crate") => Ok(Visibility::Crate),
                    Some("super") => Ok(Visibility::Super),
                    Some("restricted") => {
                        let p = path.ok_or_else(|| de::Error::missing_field("path"))?;
                        Ok(Visibility::Restricted { path: p })
                    }
                    Some(other) => Err(de::Error::unknown_variant(
                        other,
                        &["public", "private", "crate", "super", "restricted"],
                    )),
                    None => Err(de::Error::missing_field("kind")),
                }
            }
        }

        d.deserialize_any(VisVisitor)
    }
}

// ── Nested complexity sub-types ───────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Loc {
    /// sloc — lines containing source code
    #[serde(serialize_with = "sig3")]
    pub source: f64,
    /// lloc — logical lines (statements)
    #[serde(default, serialize_with = "sig3", skip_serializing_if = "is_zero_f64")]
    pub logical: f64,
    /// cloc — lines containing comments
    #[serde(default, serialize_with = "sig3", skip_serializing_if = "is_zero_f64")]
    pub comments: f64,
    /// blank lines
    #[serde(default, serialize_with = "sig3", skip_serializing_if = "is_zero_f64")]
    pub blank: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Halstead {
    #[serde(serialize_with = "sig3")]
    pub length: f64,
    #[serde(serialize_with = "sig3")]
    pub vocabulary: f64,
    #[serde(serialize_with = "sig3")]
    pub volume: f64,
    #[serde(serialize_with = "sig3")]
    pub effort: f64,
    #[serde(serialize_with = "sig3")]
    pub time: f64,
    #[serde(serialize_with = "sig3")]
    pub bugs: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Maintainability {
    #[serde(serialize_with = "sig3")]
    pub mi: f64,
    #[serde(default, serialize_with = "sig3", skip_serializing_if = "is_zero_f64")]
    pub mi_sei: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Coupling {
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub fan_in: u32,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub fan_out: u32,
    /// Outgoing edges to external libraries (depth-1 deps). Tracked separately
    /// so it is visible without inflating HK, which uses internal coupling only.
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub fan_out_external: u32,
    #[serde(default, serialize_with = "sig3", skip_serializing_if = "is_zero_f64")]
    pub hk: f64,
}

fn coupling_is_trivial(c: &Option<Coupling>) -> bool {
    c.as_ref()
        .is_none_or(|c| c.fan_in == 0 && c.fan_out == 0 && c.fan_out_external == 0)
}

fn complexity_is_empty(c: &Option<Complexity>) -> bool {
    c.as_ref().is_none_or(|c| {
        c.cyclomatic == 0.0
            && c.cognitive == 0.0
            && c.exits == 0.0
            && c.args == 0.0
            && c.functions == 0.0
            && c.closures == 0.0
            && coupling_is_trivial(&c.coupling)
            && c.maintainability.is_none()
            && c.loc.is_none()
            && c.halstead.is_none()
    })
}

/// Full complexity metrics for a node (fn/method/file/module).
/// Computed by rust-code-analysis; absent when the node has no source or
/// the file could not be parsed.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Complexity {
    #[serde(default, serialize_with = "sig3", skip_serializing_if = "is_zero_f64")]
    pub cyclomatic: f64,
    #[serde(default, serialize_with = "sig3", skip_serializing_if = "is_zero_f64")]
    pub cognitive: f64,
    #[serde(default, serialize_with = "sig3", skip_serializing_if = "is_zero_f64")]
    pub exits: f64,
    #[serde(default, serialize_with = "sig3", skip_serializing_if = "is_zero_f64")]
    pub args: f64,
    #[serde(default, serialize_with = "sig3", skip_serializing_if = "is_zero_f64")]
    pub functions: f64,
    #[serde(default, serialize_with = "sig3", skip_serializing_if = "is_zero_f64")]
    pub closures: f64,
    #[serde(default, skip_serializing_if = "coupling_is_trivial")]
    pub coupling: Option<Coupling>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maintainability: Option<Maintainability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loc: Option<Loc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub halstead: Option<Halstead>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub kind: NodeKind,
    pub name: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<NodeId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visibility: Option<Visibility>,
    /// Structural line-count for the file/module (not a complexity metric).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loc: Option<u32>,
    /// Line number where this fn/method is declared (1-based).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item_count: Option<u32>,
    /// For traits: number of method items declared on the trait.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method_count: Option<u32>,
    #[serde(default, skip_serializing_if = "complexity_is_empty")]
    pub complexity: Option<Complexity>,
    /// Set when this node is part of a cycle (SCC with ≥ 2 members).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cycle_kind: Option<CycleKind>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub from: NodeId,
    pub to: NodeId,
    pub kind: EdgeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unresolved: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visibility: Option<Visibility>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    /// All SCCs with ≥ 2 members, classified by kind.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cycles: Vec<CycleGroup>,
    /// Aggregate statistics computed after all annotations (hk, cycles) are applied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stats: Option<GraphStats>,
}

impl Graph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn project(&self, node_kinds: &[NodeKind], edge_kinds: &[EdgeKind]) -> Graph {
        let kept_ids: HashSet<&NodeId> = self
            .nodes
            .iter()
            .filter(|n| node_kinds.contains(&n.kind))
            .map(|n| &n.id)
            .collect();
        let nodes = self
            .nodes
            .iter()
            .filter(|n| node_kinds.contains(&n.kind))
            .cloned()
            .collect();
        let edges = self
            .edges
            .iter()
            .filter(|e| {
                edge_kinds.contains(&e.kind)
                    && kept_ids.contains(&e.from)
                    && kept_ids.contains(&e.to)
            })
            .cloned()
            .collect();
        Graph {
            nodes,
            edges,
            cycles: Vec::new(),
            stats: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(id: &str, kind: NodeKind) -> Node {
        Node {
            id: id.into(),
            kind,
            name: id.into(),
            path: String::new(),
            parent: None,
            external: None,
            visibility: None,
            loc: None,
            line: None,
            item_count: None,
            method_count: None,
            complexity: None,
            cycle_kind: None,
        }
    }

    fn e(from: &str, to: &str, kind: EdgeKind) -> Edge {
        Edge {
            from: from.into(),
            to: to.into(),
            kind,
            unresolved: None,
            external: None,
            visibility: None,
        }
    }

    #[test]
    fn project_keeps_only_matching_node_kinds() {
        let g = Graph {
            nodes: vec![n("c", NodeKind::Crate), n("m", NodeKind::Module)],
            edges: vec![],
            cycles: vec![],
            stats: None,
        };
        let p = g.project(&[NodeKind::Crate], &[]);
        assert_eq!(p.nodes.len(), 1);
        assert_eq!(p.nodes[0].id, "c");
    }

    #[test]
    fn project_drops_edges_to_filtered_out_nodes() {
        let g = Graph {
            nodes: vec![n("a", NodeKind::Crate), n("b", NodeKind::Module)],
            edges: vec![e("a", "b", EdgeKind::Contains)],
            cycles: vec![],
            stats: None,
        };
        let p = g.project(&[NodeKind::Crate], &[EdgeKind::Contains]);
        assert!(p.edges.is_empty());
    }

    #[test]
    fn project_keeps_edges_between_kept_nodes() {
        let g = Graph {
            nodes: vec![n("a", NodeKind::Crate), n("b", NodeKind::Crate)],
            edges: vec![e("a", "b", EdgeKind::Uses)],
            cycles: vec![],
            stats: None,
        };
        let p = g.project(&[NodeKind::Crate], &[EdgeKind::Uses]);
        assert_eq!(p.edges.len(), 1);
    }

    #[test]
    fn visibility_roundtrip_new_format() {
        let cases: &[(&str, Visibility)] = &[
            ("\"public\"", Visibility::Public),
            ("\"private\"", Visibility::Private),
            ("\"crate\"", Visibility::Crate),
            ("\"super\"", Visibility::Super),
            (
                r#"{"restricted":"some::path"}"#,
                Visibility::Restricted {
                    path: "some::path".into(),
                },
            ),
        ];
        for (json, expected) in cases {
            let got: Visibility = serde_json::from_str(json).unwrap();
            assert_eq!(got, expected.clone());
            let re = serde_json::to_string(&got).unwrap();
            let back: Visibility = serde_json::from_str(&re).unwrap();
            assert_eq!(back, expected.clone());
        }
    }

    #[test]
    fn visibility_deserialize_old_tagged_format() {
        let cases: &[(&str, Visibility)] = &[
            (r#"{"kind":"public"}"#, Visibility::Public),
            (r#"{"kind":"private"}"#, Visibility::Private),
            (r#"{"kind":"crate"}"#, Visibility::Crate),
            (r#"{"kind":"super"}"#, Visibility::Super),
            (
                r#"{"kind":"restricted","path":"some::path"}"#,
                Visibility::Restricted {
                    path: "some::path".into(),
                },
            ),
        ];
        for (json, expected) in cases {
            let got: Visibility = serde_json::from_str(json).unwrap();
            assert_eq!(got, expected.clone());
        }
    }

    #[test]
    fn complexity_coupling_roundtrip() {
        let cx = Complexity {
            cyclomatic: 3.0,
            coupling: Some(Coupling {
                fan_in: 2,
                fan_out: 4,
                hk: 576.0,
                ..Default::default()
            }),
            ..Default::default()
        };
        let json = serde_json::to_string(&cx).unwrap();
        let back: Complexity = serde_json::from_str(&json).unwrap();
        assert_eq!(back.cyclomatic, 3.0);
        let c = back.coupling.unwrap();
        assert_eq!(c.fan_in, 2);
        assert_eq!(c.fan_out, 4);
    }
}
