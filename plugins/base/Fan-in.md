# Fan-in — Afferent Coupling

**TL;DR**: Fan-in (afferent coupling) counts how many other modules depend on
this one. High fan-in modules are load-bearing: a change here ripples out to
every dependant, and a bug here is felt everywhere. The goal is not to lower
fan-in for its own sake, but to make high fan-in modules **stable** — a small,
deliberate contract that rarely needs to change.

## What it measures

`fan_in` is the number of distinct modules that depend on this one — its
incoming flow edges (import paths, qualified references). It is the
mirror of [fan-out](Fan-out.md): if A uses B, that is +1 to
A's fan-out and +1 to B's fan-in.

## Why it matters

A high-fan-in module is widely relied upon, which cuts two ways:

- **Reuse is good**: a foundational type or interface used everywhere is doing
  its job. High fan-in is expected for core abstractions.
- **Change is expensive**: every modification forces re-compilation, re-review,
  and potential breakage across all dependants. A high-fan-in module that
  *also* changes often is a serious risk.

So fan-in is read together with stability. Robert Martin's Stable Dependencies
Principle says modules should depend in the direction of stability: the things
many others lean on should be the things least likely to change.

## What high fan-in looks like

High fan-in shows up as:

- A core types/domain module every layer imports.
- A widely-used interface (e.g. a custom error type, a config contract).
- A shared "prelude" or common module pulled in across the codebase.

A breaking change to a widely-imported interface can cascade through every
implementation, which is what makes these modules especially load-bearing.

## Reducing it (or stabilising it)

For each high-fan-in module:

- **Minimise the contract**: expose the smallest public surface that callers
  actually need, keeping the rest internal. The less you expose, the less can
  break dependants.
- **Stabilise it**: prefer stable abstractions (interfaces, plain data types)
  over volatile concrete logic at the points everyone depends on.
- **Segregate it**: if different dependants use disjoint parts of the module,
  split it so each caller depends only on what it uses
  (see [ISP](ISP.md)). This lowers fan-in on each
  resulting piece and shrinks the blast radius of a change.

## How code-ranker surfaces it

`fan_in` is a first-class node metric, a sort option, and the `FANIN` principle
in the Prompt Generator. The principle ranks modules by fan-in worst-first and
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
