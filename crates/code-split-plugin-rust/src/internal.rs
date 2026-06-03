//! Internal intermediate graph model used during parsing. The parsing stages
//! (crate_graph, module_graph) build these; `collapse_to_files` converts them
//! to the public `code_split_plugin_api::Graph`.

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
    /// Convert to the string form the plugin API expects.
    pub(crate) fn as_str(&self) -> &str {
        match self {
            Visibility::Public => "public",
            Visibility::Crate => "crate",
            Visibility::Super => "super",
            Visibility::Private => "private",
            Visibility::Restricted { path } => path.as_str(),
        }
    }
}

pub(crate) type NodeId = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EdgeKind {
    Contains,
    Uses,
    Reexports,
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
}

#[derive(Debug, Clone)]
pub(crate) struct Edge {
    pub from: NodeId,
    pub to: NodeId,
    pub kind: EdgeKind,
    pub visibility: Option<Visibility>,
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
