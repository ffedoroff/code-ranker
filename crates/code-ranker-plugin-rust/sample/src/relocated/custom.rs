//! Reached only via `#[path = "relocated/custom.rs"] mod relocated;` in lib.rs —
//! a module whose backing file is at a non-default location. The analyzer
//! honours `#[path]`, so this file is walked and its dependency is captured:
//!   relocated/custom.rs → c.rs
//! Without `#[path]` support the file (and this edge) would be silently dropped.

use crate::c::gamma;

pub fn relocated_sum() -> i32 {
    gamma() + 1
}
