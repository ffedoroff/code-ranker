//! Rust sample fixture for code-split.
//!
//! Goal: exercise every file→file dependency form the analyzer DOES detect,
//! plus the known blind spots it does NOT (yet) detect. The analyzer is
//! `syn`-based: it walks `Item::Use` and `Item::Mod`, and also collects
//! bare qualified paths (`foo::run()`, `crate::a::Alpha`) from expressions and
//! types. Macros are never expanded.

// `mod foo;` (file-backed module) — each becomes a File node. The declaration
// itself is emitted as a `Contains` edge (lib.rs → child): kept in the JSON
// snapshot as structural ownership, but NOT drawn on the main map and NOT
// counted in fan_in / HK / cycles. It is metadata, not information flow.
#[macro_use]
mod macros;
pub mod a;
pub mod b;
pub mod c;
mod foo;

// `pub use` re-export — DETECTED as a `Reexports` edge (lib.rs → a.rs).
pub use crate::a::Alpha;

// Intra-crate bare-path call: lib.rs calls `foo::run()` by a BARE PATH (no
// `use crate::foo`). This IS captured as a `Uses` edge (lib.rs → foo.rs) — bare
// `mod::item` references resolve against the local module index. So foo.rs gets
// a real inbound `Uses` edge in addition to the structural `Contains`.
pub fn run_foo() -> i32 {
    foo::run()
}

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
