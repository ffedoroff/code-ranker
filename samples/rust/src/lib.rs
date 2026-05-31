//! Rust sample fixture for code-split.
//!
//! Goal: exercise every file→file dependency form the analyzer DOES detect,
//! plus the known blind spots it does NOT (yet) detect. The analyzer is
//! `syn`-based and walks only `Item::Use` and `Item::Mod`; macros are never
//! expanded.

// `mod foo;` (file-backed module) — DETECTED. Each becomes a File node, and the
// `Contains` relation is collapsed away in the file graph.
#[macro_use]
mod macros;
pub mod a;
pub mod b;
pub mod c;

// `pub use` re-export — DETECTED as a `Reexports` edge (lib.rs → a.rs).
pub use crate::a::Alpha;

// `extern crate` (old 2015-style) — NOT detected. syn parses it as
// `Item::ExternCrate`, which the analyzer ignores, so no edge to `serde` comes
// from here (the `use serde::...` in a.rs is what actually surfaces serde).
extern crate serde;

// Item-position macro invocation — NOT detected. Expands to a function item,
// but the analyzer never sees inside it: no node, no edge.
make_answer!();

#[cfg(test)]
mod tests {
    // `use` inside an inline module — DETECTED (collapses into lib.rs's file).
    use crate::a;
    use crate::b;

    #[test]
    fn smoke() {
        assert_eq!(a::alpha() + b::beta(), 3);
    }
}
