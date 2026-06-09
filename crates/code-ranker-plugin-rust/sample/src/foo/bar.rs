//! Submodule `foo::bar` (file-backed at `src/foo/bar.rs`).
//!
//! `use super::*` — DETECTED as a `super` edge (bar.rs → foo.rs): a glob `use`
//! that pulls in the *enclosing* module's namespace. Like `use crate::<ancestor>::*`,
//! it is structural scope-sugar — a module split across files reaching back into
//! itself — NOT a real outward dependency. So, like `contains` / `reexports`, it
//! is kept in the JSON snapshot but excluded from fan_in / fan_out / HK / cycles
//! and NOT drawn on the main map.
//!
//! Note the contrast with `b.rs`, whose `use super::a::alpha` is a *named* import
//! of a sibling item and therefore a real `Uses` edge — only the glob namespace
//! pull from an ancestor becomes `super`.
use super::*;

/// Uses `run` brought into scope by `use super::*` (a bare 1-segment call, so it
/// adds no qualified-path edge — the only bar.rs → foo.rs edge is the `super` glob).
pub fn nested() -> i32 {
    run() + 1
}
