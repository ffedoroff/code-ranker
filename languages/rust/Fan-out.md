# Fan-out — Efferent Coupling (in Rust)

**TL;DR**: Fan-out (efferent coupling) counts how many other modules this one
depends on. High fan-out makes a module fragile — it breaks when any of its
many dependencies change — and hard to test or reuse in isolation. Reduce it
by depending on fewer, more abstract collaborators.

## What it measures

`fan_out` is the number of distinct modules this one depends on — its outgoing
flow edges (`use` paths, qualified references, derives). External-library
dependencies are tracked separately (`fan_out_external`) and not counted here.
Fan-out is the mirror of [fan-in](Fan-in.md).
<!-- doc:base "Why it matters" -->

## In Rust

High fan-out typically appears in orchestration code:

- An application `main` / service-wiring module that touches every subsystem.
- A "manager" or "coordinator" that pulls in many concrete collaborators.
- A handler that reaches directly into persistence, validation, formatting,
  and external clients all at once.

Some fan-out is inherent at composition roots — that is where wiring lives.
The concern is fan-out in modules that are supposed to hold focused logic.

## Reducing it

For each high-fan-out module:

- **Depend on abstractions**: replace several concrete collaborators with a
  trait the module owns, and inject implementations
  (see [DIP](DIP.md)). The module then depends on one
  abstraction instead of N concretes.
- **Collapse fine-grained dependencies**: if it talks to several small modules
  that always travel together, hide them behind one focused interface.
- **Move misplaced logic**: code that drags in unrelated imports usually
  belongs in a module closer to those dependencies
  (see [LoD](LoD.md) — talk to immediate collaborators, not the
  whole graph).
<!-- doc:base "How code-ranker surfaces it" -->
<!-- doc:base "Related principles" -->
<!-- doc:base "References" -->
