# Fan-in — Afferent Coupling (in Rust)

**TL;DR**: Fan-in (afferent coupling) counts how many other modules depend on
this one. High fan-in modules are load-bearing: a change here ripples out to
every dependant, and a bug here is felt everywhere. The goal is not to lower
fan-in for its own sake, but to make high fan-in modules **stable** — a small,
deliberate contract that rarely needs to change.

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

## How code-ranker surfaces it

`fan_in` is a first-class node metric, a sort option, and the `FANIN` preset
in the Prompt Generator. The preset ranks modules by fan-in worst-first and
pre-selects **incoming** connections, so the prompt shows who depends on each
load-bearing module.

## Related principles

- [ISP](ISP.md) — split a widely-used module so
  callers depend only on the slice they need.
- [DIP](DIP.md) — depend on stable abstractions, which
  is what high-fan-in modules should be.
- [Fan-out](Fan-out.md) — the outgoing-dependency mirror.

## References

1. Martin, R. C. "Design Principles and Design Patterns" (Stable Dependencies
   / Stable Abstractions Principles). 2000.
