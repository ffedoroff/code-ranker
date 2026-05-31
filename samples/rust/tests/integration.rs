//! Integration test for the Rust sample.
//!
//! NOTE: files under `tests/` compile as a separate Cargo target of kind
//! `test`. The analyzer only walks `lib`/`bin`/`proc-macro`-style targets, so
//! this file is NOT analyzed at all — it never appears as a node in the report,
//! regardless of the `ignore.tests` config (which acts later, on already-built
//! file graphs). This is the Rust counterpart to the other languages' test
//! files, and a deliberate blind spot.

use rust_sample::a;
use rust_sample::b;

#[test]
fn alpha_plus_beta() {
    assert_eq!(a::alpha() + b::beta(), 3);
}
