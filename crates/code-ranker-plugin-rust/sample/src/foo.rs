//! Module `foo` — the intra-crate bare-path case.
//!
//! `foo` is reached through its `mod foo;` declaration in lib.rs (a structural
//! `Contains` edge) AND a bare-path call `foo::run()` with NO `use crate::foo`.
//! That bare-path call IS captured as a `Uses` edge (lib.rs → foo.rs), so foo.rs
//! has a real inbound edge (fan_in 1) — the `Contains` is excluded from fan_in.
//!
//! `foo` itself `use`s `b`, so it also has an outgoing edge (`foo.rs → b.rs`).

use crate::b::beta;

// File-backed submodule `foo::bar` (at `src/foo/bar.rs`). Its `use super::*`
// pulls this module's namespace back in — captured as a `super` edge
// (bar.rs → foo.rs), the glob-namespace-pull-from-an-ancestor case.
pub mod bar;

/// Called from lib.rs via the bare path `foo::run()` (no `use crate::foo`).
pub fn run() -> i32 {
    beta() + 1
}
