//! Internal intermediate graph model used during parsing. The parsing stages
//! (crate_graph, module_graph) build these; `collapse_to_files` converts them
//! to the public `code_ranker_plugin_api::graph::Graph`.

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NodeKind {
    Crate,
    Module,
}

/// Visibility of a module / re-export, derived from `syn::Visibility`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Visibility {
    Public,
    Crate,
    Super,
    Private,
    Restricted { path: String },
}

impl Visibility {
    /// Convert to the string form the plugin API expects. The output strings are
    /// DATA (`[visibility]` in `rust/config.toml`); the variant→slot LOGIC stays
    /// here. `Restricted` emits its path verbatim (no fixed vocabulary string).
    pub(crate) fn as_str(&self) -> &str {
        match self {
            Visibility::Public => super::cfg::visibility("public"),
            Visibility::Crate => super::cfg::visibility("crate"),
            Visibility::Super => super::cfg::visibility("super"),
            Visibility::Private => super::cfg::visibility("private"),
            Visibility::Restricted { path } => path.as_str(),
        }
    }
}

pub(crate) type NodeId = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum EdgeKind {
    Contains,
    Uses,
    Reexports,
    /// A glob `use` that pulls in an *enclosing* module's namespace
    /// (`use super::*`, `use crate::<ancestor>::*`). Structural scope-sugar, not a
    /// real outward dependency — treated like `Contains` (kept, not drawn,
    /// excluded from fan-in/out/HK/cycles).
    Super,
}

#[derive(Debug, Clone)]
pub(crate) struct Node {
    pub id: NodeId,
    pub kind: NodeKind,
    pub name: String,
    /// Absolute filesystem path (for Module: path of the file it lives in;
    /// for Crate: path of its Cargo.toml).
    pub path: String,
    pub parent: Option<NodeId>,
    pub external: Option<bool>,
    pub version: Option<String>,
    pub visibility: Option<Visibility>,
    pub loc: Option<u32>,
    /// Some(line) → inline module; None → file-backed module.
    pub line: Option<u32>,
    pub item_count: Option<u32>,
    /// Count of `unsafe` usages in the file's production code (`unsafe { }`
    /// blocks plus `unsafe fn`/`impl`/`trait` declarations); `None` until the
    /// file is walked. Test items (`#[cfg(test)]`/`#[test]`/`#[bench]`) excluded.
    pub unsafe_count: Option<u32>,
    /// Human-readable owning-crate label (compilation unit), e.g. `bat` or
    /// `bat (bin)`. `None` for crate / external nodes. A package can expose
    /// several crates (a lib and one or more bins), so this is per-target.
    pub crate_label: Option<String>,
    /// Syntactic facts from the file's production AST (test items excluded),
    /// each a sorted comma-joined set — for config `[rules.checks]` predicates
    /// (`contains(derives, "Serialize")`, `contains(macros, "println")`, …).
    pub facts: Facts,
}

/// Per-file syntactic fact sets, emitted as comma-joined string node attributes.
/// Empty (`None`) sets are not emitted. Collected over the production AST only.
#[derive(Debug, Clone, Default)]
pub(crate) struct Facts {
    /// `#[derive(...)]` names (last path segment), e.g. `Serialize,Debug`.
    pub derives: Option<String>,
    /// Macro invocation names (last path segment), e.g. `println,vec`.
    pub macros: Option<String>,
    /// Attribute names other than `derive` (last path segment), e.g. `tokio,serde`.
    pub attrs: Option<String>,
    /// Qualified paths referenced (≥2 segments), e.g. `http::StatusCode,std::fmt`.
    pub imports: Option<String>,
    /// Names of types defined in the file (struct / enum / type alias).
    pub types: Option<String>,
    /// Names of traits defined in the file.
    pub traits: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct Edge {
    pub from: NodeId,
    pub to: NodeId,
    pub kind: EdgeKind,
    pub visibility: Option<Visibility>,
    /// 1-based line of the `use` statement this edge came from; `None` for
    /// structural (`contains`) or bare-path edges.
    pub line: Option<u32>,
}

#[derive(Debug, Default)]
pub(crate) struct InternalGraph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

/// Append-only builder — equivalent to the old `GraphBuilder`.
#[derive(Debug, Default)]
pub(crate) struct GraphBuilder {
    graph: InternalGraph,
}

impl GraphBuilder {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn add_node(&mut self, node: Node) {
        self.graph.nodes.push(node);
    }

    pub(crate) fn add_edge(&mut self, edge: Edge) {
        self.graph.edges.push(edge);
    }

    /// Mutable access to the accumulated nodes (used by `aggregate_crate_loc`).
    pub(crate) fn nodes_mut(&mut self) -> &mut Vec<Node> {
        &mut self.graph.nodes
    }

    pub(crate) fn build(self) -> InternalGraph {
        self.graph
    }
}

#[cfg(test)]
#[path = "tests/internal.rs"]
mod internal_tests;
