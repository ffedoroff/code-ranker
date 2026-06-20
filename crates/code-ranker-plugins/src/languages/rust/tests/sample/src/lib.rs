//! Rust sample fixture for code-ranker.
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
// `cross` depends on the `helper` workspace member by SUBMODULE path; `derives`
// depends on serde only through a qualified derive (see those files).
pub mod cross;
// `complex` — exercises the per-function complexity metrics (cyclomatic,
// cognitive, exits, args, closures) and the `unsafe` count, so the golden guards
// them with real non-trivial values rather than the omit-value floor.
pub mod complex;
// `chain` — a 3-node `uses` cycle (one → two → three → one) so the golden covers
// the `chain` cycle kind, next to the 2-node `mutual` cycle from `a ⇄ b`.
pub mod chain;
// `cycle_examples` — self-contained demo of which edge forms close a cycle and
// which do not (uses/contains/reexports/super); see docs/cycles.md.
pub mod cycle_examples;
pub mod derives;
mod foo;

// `#[path = "..."]` module — its backing file lives at a non-default location
// (`src/relocated/custom.rs`). DETECTED: the analyzer honours `#[path]`, walks
// the file, and captures its edges (`custom.rs → c.rs`). Without `#[path]`
// support the whole file and its edges would be silently dropped.
#[path = "relocated/custom.rs"]
mod relocated;

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
