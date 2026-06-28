## What it measures

`fan_in` is the number of distinct modules that depend on this one — its
incoming flow edges (`use` paths, qualified references, derives). It is the
mirror of [fan-out](Fan-out.md): if A uses B, that is +1 to
A's fan-out and +1 to B's fan-in.

## Why it matters

A high-fan-in module is widely relied upon, which cuts two ways:

- **Reuse is good**: a foundational type or trait used everywhere is doing its
  job. High fan-in is expected for core abstractions.
- **Change is expensive**: every modification forces recompilation, re-review,
  and potential breakage across all dependants. A high-fan-in module that
  *also* changes often is a serious risk.

So fan-in is read together with stability. Robert Martin's Stable Dependencies
Principle says modules should depend in the direction of stability: the things
many others lean on should be the things least likely to change.

## In Rust

High fan-in shows up as:

- A core `types.rs` / domain crate every layer imports.
- A widely-derived trait (e.g. a custom `Error`, a `Config`).
- A `prelude` module pulled in across the codebase.

Rust's orphan rules and coherence make these especially load-bearing: a
breaking change to a widely-imported trait can cascade through every `impl`.

## Reducing it (or stabilising it)

For each high-fan-in module:

- **Minimise the contract**: expose the smallest public surface that callers
  actually need (`pub(crate)` / `pub(super)` for the rest). The less you
  expose, the less can break dependants.
- **Stabilise it**: prefer stable abstractions (traits, plain data types) over
  volatile concrete logic at the points everyone depends on.
- **Segregate it**: if different dependants use disjoint parts of the module,
  split it so each caller depends only on what it uses
  (see [ISP](ISP.md)). This lowers fan-in on each
  resulting piece and shrinks the blast radius of a change.
<!-- doc:base "How code-ranker surfaces it" -->
<!-- doc:base "Related principles" -->
<!-- doc:base "References" -->
