//! Rust sample fixture for code-split.
//!
//! Goal: exercise every file→file dependency form the analyzer DOES detect,
//! plus the known blind spots it does NOT (yet) detect. The analyzer is
//! `syn`-based and walks only `Item::Use` and `Item::Mod`; macros are never
//! expanded.

// `mod foo;` (file-backed module) — each becomes a File node, but the
// declaration is NOT a dependency edge: it is structural ownership, not an
// import. (A child reached ONLY via `mod foo;` + a bare-path call therefore has
// no inbound edge — see foo.rs / macros.rs.)
#[macro_use]
mod macros;
pub mod a;
pub mod b;
pub mod c;
mod foo;

// `pub use` re-export — DETECTED as a `Reexports` edge (lib.rs → a.rs).
pub use crate::a::Alpha;

// The canonical `mod foo;` case: lib.rs calls `foo::run()` by a BARE PATH (no
// `use crate::foo`). The bare-path call is not captured, so the `lib.rs → foo.rs`
// edge comes solely from the `mod foo;` declaration above.
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
