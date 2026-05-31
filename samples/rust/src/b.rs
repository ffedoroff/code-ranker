//! Module `b` — imports `a`, completing the a ⇄ b cycle. Also home to two
//! blind spots: a fully-qualified external path with no `use`, and a dependency
//! hidden inside a macro invocation.

// `use super::...` — DETECTED (b.rs → a.rs). Completes the a ⇄ b cycle.
use super::a::alpha;

pub fn beta() -> i32 {
    // A `println!` invocation inside a function body — NOT detected (std macro,
    // never recorded, and std is not an external node anyway).
    println!("alpha is {}", alpha());

    // Fully-qualified external path with NO `use` statement — NOT detected.
    // `once_cell` is a real dependency, but because it is never `use`d, no edge
    // to it is produced: it must be absent from the External nodes.
    let cell: once_cell::sync::Lazy<i32> = once_cell::sync::Lazy::new(|| 2);
    *cell
}

pub fn beta_via_macro() -> i32 {
    // The `pull_in_c!()` macro expands to `use crate::c::gamma; gamma()`. Because
    // syn does not expand macros, the `use crate::c::gamma` hidden in its body is
    // INVISIBLE — no edge b.rs → c.rs is produced from here.
    pull_in_c!()
}
