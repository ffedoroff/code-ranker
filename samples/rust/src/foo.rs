//! Module `foo` — a "no inbound edge" blind-spot case.
//!
//! `foo` is reached ONLY through its `mod foo;` declaration in lib.rs plus a
//! bare-path call `foo::run()` (NO `use crate::foo`). A `mod` declaration is
//! structural (not an edge) and the bare-path call is not captured, so nothing
//! points at `foo.rs` — it has **no inbound edge** (fan_in 0).
//!
//! `foo` itself `use`s `b`, so it still has an outgoing edge (`foo.rs → b.rs`).

use crate::b::beta;

/// Called from lib.rs via the bare path `foo::run()` (no `use crate::foo`).
pub fn run() -> i32 {
    beta() + 1
}
