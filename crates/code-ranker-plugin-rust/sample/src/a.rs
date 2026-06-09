//! Module `a` — depends on `b` and `c`; `b` depends back on `a` → an a ⇄ b cycle.

// Simple path use — DETECTED (a.rs → b.rs).
use crate::b::beta;
// Grouped use — DETECTED (a.rs → c.rs), one edge per resolved target.
use crate::c::{gamma, helpers};
// Glob use — DETECTED (resolves to the module, a.rs → c.rs again, deduped).
use crate::c::*;
// Renamed use — DETECTED.
use crate::b::beta as b_beta;

// External crate via `use` + a derive macro. The `use serde::Serialize` is what
// the analyzer keys on → serde becomes an External node (depth 1). The
// `#[derive(Serialize)]` attribute itself is not a dependency signal.
use serde::Serialize;

// Standard library use — DETECTED as an import but std/core/alloc are NOT
// emitted as External nodes (they are not third-party dependencies).
use std::collections::HashMap;

#[derive(Serialize)]
pub struct Alpha {
    pub n: i32,
}

pub fn alpha() -> i32 {
    let _seen: HashMap<i32, i32> = HashMap::new();
    // Calls into `b` and `c`; the `use` edges above are what the graph records.
    1 + beta() - b_beta() + gamma() - gamma() + helpers::offset() - helpers::offset()
}
