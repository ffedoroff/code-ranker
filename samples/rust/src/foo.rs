//! Module `foo` — the canonical `mod foo;` case.
//!
//! `foo` is reached ONLY through its `mod foo;` declaration in lib.rs: lib.rs
//! calls `foo::run()` by a bare path, with NO `use crate::foo`. The `syn`-based
//! analyzer never captures that bare-path call, so the single thing that
//! surfaces the `lib.rs → foo.rs` dependency in the file graph is the module
//! declaration itself (collapsed from a `Contains` relation into a `uses` edge).
//!
//! `foo` in turn `use`s `b`, so it also has a normal outgoing edge
//! (`foo.rs → b.rs`); together that gives it both fan-in and fan-out, so HK is
//! non-zero.

use crate::b::beta;

/// Called from lib.rs via the bare path `foo::run()` (no `use crate::foo`).
pub fn run() -> i32 {
    beta() + 1
}
