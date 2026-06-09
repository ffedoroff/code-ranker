//! [`Graph`] — what a plugin's `analyze` returns: pure **structure**, nodes +
//! edges, and nothing computed.
//!
//! Derived data is produced centrally afterwards and lives outside this type:
//! - per-node metrics and cycle membership are written into nodes' `attrs` by id
//!   during enrichment (in the metrics/graph layer, not in this contract);
//! - graph-level cycles and averages are computed by the orchestrator and stored
//!   in the snapshot, not here.

use crate::edge::Edge;
use crate::node::Node;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}
